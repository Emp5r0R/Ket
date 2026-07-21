use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ket_core::CreateAccessGrantRequest;
use ket_server::{
    AccessService,
    service::{CreatedSession, ServiceError},
};
use tokio::sync::Barrier;

const NODE_CAPACITY: u32 = 4;

struct TemporaryState {
    path: PathBuf,
}

impl TemporaryState {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "ket-concurrent-lifecycle-{}-{nonce}.json",
                std::process::id()
            )),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryState {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("json.tmp"));
    }
}

async fn create_sessions_together(
    service: Arc<AccessService>,
    access_code: String,
    client_prefix: &str,
    count: usize,
) -> Vec<Result<CreatedSession, ServiceError>> {
    let barrier = Arc::new(Barrier::new(count + 1));
    let mut tasks = Vec::with_capacity(count);

    for index in 0..count {
        let service = Arc::clone(&service);
        let access_code = access_code.clone();
        let client_name = format!("{client_prefix}-{index}");
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            service.create_session(&access_code, client_name).await
        }));
    }

    barrier.wait().await;
    let mut results = Vec::with_capacity(count);
    for task in tasks {
        results.push(task.await.expect("session admission task should not panic"));
    }
    results
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_multi_code_sessions_respect_limits_and_survive_restart() {
    let temporary_state = TemporaryState::new();
    let service = Arc::new(
        AccessService::load_from_file(
            temporary_state.path(),
            Duration::from_secs(300),
            NODE_CAPACITY,
        )
        .await
        .expect("service should load"),
    );

    let personal = service
        .create_grant(CreateAccessGrantRequest {
            label: "Personal".to_owned(),
            max_connections: 2,
            expires_at_epoch_seconds: None,
        })
        .await
        .expect("personal grant should be created");
    let personal_results = create_sessions_together(
        Arc::clone(&service),
        personal.access_code.expose_secret().to_owned(),
        "personal",
        4,
    )
    .await;

    assert_eq!(
        personal_results
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        2
    );
    assert_eq!(
        personal_results
            .iter()
            .filter(|result| matches!(result, Err(ServiceError::GrantCapacity)))
            .count(),
        2
    );

    let fleet = service
        .create_grant(CreateAccessGrantRequest {
            label: "Fleet".to_owned(),
            max_connections: 3,
            expires_at_epoch_seconds: None,
        })
        .await
        .expect("fleet grant should be created");
    let fleet_results = create_sessions_together(
        Arc::clone(&service),
        fleet.access_code.expose_secret().to_owned(),
        "fleet",
        4,
    )
    .await;

    assert_eq!(
        fleet_results.iter().filter(|result| result.is_ok()).count(),
        2
    );
    assert_eq!(
        fleet_results
            .iter()
            .filter(|result| matches!(result, Err(ServiceError::NodeCapacity)))
            .count(),
        2
    );
    assert_eq!(service.active_session_count().await, NODE_CAPACITY);

    let sessions: Vec<_> = personal_results
        .into_iter()
        .chain(fleet_results)
        .filter_map(Result::ok)
        .collect();
    let session_ids: HashSet<_> = sessions.iter().map(|session| session.id.as_str()).collect();
    let control_tokens: HashSet<_> = sessions
        .iter()
        .map(|session| session.token.expose_secret())
        .collect();
    let data_plane_tokens: HashSet<_> = sessions
        .iter()
        .map(|session| session.data_plane_token.expose_secret())
        .collect();
    assert_eq!(session_ids.len(), NODE_CAPACITY as usize);
    assert_eq!(control_tokens.len(), NODE_CAPACITY as usize);
    assert_eq!(data_plane_tokens.len(), NODE_CAPACITY as usize);

    let encoded_state = fs::read_to_string(temporary_state.path()).expect("state should be stored");
    let state: serde_json::Value =
        serde_json::from_str(&encoded_state).expect("stored state should remain valid JSON");
    assert_eq!(state["grants"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        state["sessions"].as_array().map(Vec::len),
        Some(NODE_CAPACITY as usize)
    );
    for session in &sessions {
        assert!(!encoded_state.contains(session.token.expose_secret()));
        assert!(!encoded_state.contains(session.data_plane_token.expose_secret()));
    }
    assert!(!encoded_state.contains(personal.access_code.expose_secret()));
    assert!(!encoded_state.contains(fleet.access_code.expose_secret()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = fs::metadata(temporary_state.path())
            .expect("state metadata should be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    drop(service);
    let reloaded = Arc::new(
        AccessService::load_from_file(
            temporary_state.path(),
            Duration::from_secs(300),
            NODE_CAPACITY,
        )
        .await
        .expect("persisted service should reload"),
    );
    assert_eq!(reloaded.active_session_count().await, NODE_CAPACITY);

    let renewal_tasks: Vec<_> = sessions
        .iter()
        .map(|session| {
            let reloaded = Arc::clone(&reloaded);
            let token = session.token.expose_secret().to_owned();
            tokio::spawn(async move { reloaded.renew_session(&token).await })
        })
        .collect();
    for task in renewal_tasks {
        task.await
            .expect("renewal task should not panic")
            .expect("persisted session should renew");
    }

    let personal_session_ids: HashSet<_> = sessions
        .iter()
        .filter(|session| session.client_name.starts_with("personal-"))
        .map(|session| session.id.as_str())
        .collect();
    let revoked = reloaded
        .revoke_grant(&personal.id)
        .await
        .expect("personal grant should be revoked");
    assert_eq!(revoked.len(), personal_session_ids.len());
    assert!(
        revoked
            .iter()
            .all(|session| personal_session_ids.contains(session.id.as_str()))
    );
    assert_eq!(reloaded.active_session_count().await, 2);

    for session in &sessions {
        let result = reloaded.session(session.token.expose_secret()).await;
        if personal_session_ids.contains(session.id.as_str()) {
            assert!(matches!(result, Err(ServiceError::Unauthorized)));
        } else {
            result.expect("unrelated fleet session should remain active");
        }
    }

    reloaded
        .revoke_grant(&fleet.id)
        .await
        .expect("fleet grant should be revoked");
    assert_eq!(reloaded.active_session_count().await, 0);
    drop(reloaded);

    let empty_reload = AccessService::load_from_file(
        temporary_state.path(),
        Duration::from_secs(300),
        NODE_CAPACITY,
    )
    .await
    .expect("revoked state should reload");
    assert_eq!(empty_reload.active_session_count().await, 0);
    assert!(empty_reload.list_grants().await.is_empty());
    for session in sessions {
        assert!(matches!(
            empty_reload.session(session.token.expose_secret()).await,
            Err(ServiceError::Unauthorized)
        ));
    }
}
