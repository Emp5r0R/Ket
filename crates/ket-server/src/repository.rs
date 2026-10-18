use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{CURRENT_STATE_SCHEMA_VERSION, PersistedState};

const MAX_STATE_FILE_BYTES: usize = 64 * 1024 * 1024;
const LEGACY_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("state I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("state data is invalid: {0}")]
    InvalidData(#[from] serde_json::Error),
    #[error("unsupported state schema version {0}")]
    UnsupportedSchema(u32),
    #[error("state file exceeds the {MAX_STATE_FILE_BYTES}-byte limit")]
    TooLarge,
    #[error("state records are invalid: {0}")]
    InvalidState(&'static str),
    #[error("state operation failed to join: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[async_trait]
pub(crate) trait StateRepository: Send + Sync {
    async fn load(&self) -> Result<PersistedState, RepositoryError>;
    async fn store(&self, state: &PersistedState) -> Result<(), RepositoryError>;
}

#[derive(Clone, Debug)]
pub struct FileStateRepository {
    path: PathBuf,
}

impl FileStateRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl StateRepository for FileStateRepository {
    async fn load(&self) -> Result<PersistedState, RepositoryError> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || load_file(&path)).await?
    }

    async fn store(&self, state: &PersistedState) -> Result<(), RepositoryError> {
        let path = self.path.clone();
        let state = state.clone();
        tokio::task::spawn_blocking(move || store_file(&path, &state)).await?
    }
}

fn load_file(path: &Path) -> Result<PersistedState, RepositoryError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_STATE_FILE_BYTES as u64 => {
            return Err(RepositoryError::TooLarge);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersistedState::default());
        }
        Err(error) => return Err(error.into()),
    }
    let content = fs::read(path)?;
    if content.len() > MAX_STATE_FILE_BYTES {
        return Err(RepositoryError::TooLarge);
    }
    let mut state: PersistedState = serde_json::from_slice(&content)?;
    match state.schema_version {
        LEGACY_STATE_SCHEMA_VERSION => {
            state.schema_version = CURRENT_STATE_SCHEMA_VERSION;
        }
        CURRENT_STATE_SCHEMA_VERSION => {}
        unsupported => return Err(RepositoryError::UnsupportedSchema(unsupported)),
    }
    state.validate().map_err(RepositoryError::InvalidState)?;
    Ok(state)
}

fn store_file(path: &Path, state: &PersistedState) -> Result<(), RepositoryError> {
    if state.schema_version != CURRENT_STATE_SCHEMA_VERSION {
        return Err(RepositoryError::UnsupportedSchema(state.schema_version));
    }
    state.validate().map_err(RepositoryError::InvalidState)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary_path = path.with_extension("json.tmp");
    let encoded = serde_json::to_vec_pretty(state)?;
    if encoded.len() > MAX_STATE_FILE_BYTES {
        return Err(RepositoryError::TooLarge);
    }

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary_path)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::rename(&temporary_path, path)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::model::{AccessGrantRecord, SessionRecord};

    const VALID_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct TemporaryStatePath(PathBuf);

    impl TemporaryStatePath {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "ket-state-{label}-{}-{nonce}.json",
                std::process::id()
            )))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryStatePath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(self.0.with_extension("json.tmp"));
        }
    }

    #[test]
    fn legacy_v1_sessions_without_scoped_hashes_remain_loadable() {
        let path = TemporaryStatePath::new("legacy");
        let document = json!({
            "schema_version": 1,
            "grants": [{
                "id": "Grant00001",
                "secret_hash": VALID_HASH,
                "label": "Legacy grant",
                "max_connections": 1,
                "expires_at_epoch_seconds": null,
                "created_at_epoch_seconds": 100
            }],
            "sessions": [{
                "id": "Session00001",
                "grant_id": "Grant00001",
                "secret_hash": VALID_HASH,
                "client_name": "Legacy client",
                "issued_at_epoch_seconds": 100,
                "expires_at_epoch_seconds": 200
            }]
        });
        fs::write(
            path.path(),
            serde_json::to_vec(&document).expect("legacy document should encode"),
        )
        .expect("legacy state should be written");

        let state = load_file(path.path()).expect("compatible v1 state should load");

        assert_eq!(state.schema_version, CURRENT_STATE_SCHEMA_VERSION);
        assert_eq!(state.grants.len(), 1);
        assert_eq!(state.sessions.len(), 1);
        assert!(state.sessions[0].secret_hash.is_some());
        assert_eq!(state.sessions[0].control_secret_hash, None);
        assert_eq!(state.sessions[0].data_plane_secret_hash, None);

        store_file(path.path(), &state).expect("migrated state should be persisted as v2");
        let stored: serde_json::Value = serde_json::from_slice(
            &fs::read(path.path()).expect("migrated state should be readable"),
        )
        .expect("migrated state should remain JSON");
        assert_eq!(stored["schema_version"], CURRENT_STATE_SCHEMA_VERSION);
    }

    #[test]
    fn fast_control_token_hashes_round_trip_without_a_legacy_argon_hash() {
        let path = TemporaryStatePath::new("fast-control-hash");
        let mut state = valid_state();
        state.sessions[0].secret_hash = None;
        state.sessions[0].control_secret_hash = Some([7; 32]);

        store_file(path.path(), &state).expect("fast-hash state should be stored");
        let loaded = load_file(path.path()).expect("fast-hash state should load");

        assert_eq!(loaded.sessions[0].secret_hash, None);
        assert_eq!(loaded.sessions[0].control_secret_hash, Some([7; 32]));
    }

    #[test]
    fn pauses_and_protocol_limits_persist_and_default_grants_keep_their_old_shape() {
        let path = TemporaryStatePath::new("grant-controls");
        let mut state = valid_state();
        store_file(path.path(), &state).expect("default grant should be stored");
        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(path.path()).expect("state should be readable"))
                .expect("state should be JSON");
        assert!(stored["grants"][0].get("paused").is_none());
        assert!(stored["grants"][0].get("protocols").is_none());
        assert!(stored["grants"][0].get("allowance").is_none());
        assert!(stored["grants"][0].get("usage").is_none());

        state.grants[0].paused = true;
        state.grants[0].protocols =
            ket_core::ProtocolAccess::Only([ket_core::TransportProtocol::Stealth].into());
        store_file(path.path(), &state).expect("controlled grant should be stored");
        let loaded = load_file(path.path()).expect("controlled grant should load");
        assert!(loaded.grants[0].paused);
        assert_eq!(loaded.grants[0].protocols, state.grants[0].protocols);

        state.grants[0].allowance = Some(ket_core::DataAllowance {
            bytes: 10 << 30,
            period: ket_core::AllowancePeriod::Month,
        });
        state.grants[0].usage = Some(crate::model::PeriodUsage {
            period_start_epoch_seconds: 1_790_812_800,
            used_bytes: 3 << 30,
        });
        store_file(path.path(), &state).expect("allowance should be stored");
        let loaded = load_file(path.path()).expect("allowance should load");
        assert_eq!(loaded.grants[0].allowance, state.grants[0].allowance);
        assert_eq!(loaded.grants[0].usage, state.grants[0].usage);
    }

    #[test]
    fn duplicate_and_orphan_records_are_rejected_before_storage() {
        let path = TemporaryStatePath::new("relationships");
        let mut duplicate = valid_state();
        duplicate.grants.push(duplicate.grants[0].clone());
        fs::write(
            path.path(),
            serde_json::to_vec(&duplicate).expect("duplicate state should encode"),
        )
        .expect("duplicate state should be written");
        assert!(matches!(
            load_file(path.path()),
            Err(RepositoryError::InvalidState(
                "access grant IDs are duplicated"
            ))
        ));
        fs::remove_file(path.path()).expect("duplicate state should be removed");

        assert!(matches!(
            store_file(path.path(), &duplicate),
            Err(RepositoryError::InvalidState(
                "access grant IDs are duplicated"
            ))
        ));
        assert!(!path.path().exists());

        let mut orphan = valid_state();
        orphan.sessions[0].grant_id = "Other00001".to_owned();
        assert!(matches!(
            store_file(path.path(), &orphan),
            Err(RepositoryError::InvalidState(
                "session references an unknown access grant"
            ))
        ));
        assert!(!path.path().exists());
    }

    #[test]
    fn unknown_schemas_and_oversized_files_fail_closed() {
        let schema_path = TemporaryStatePath::new("schema");
        fs::write(
            schema_path.path(),
            br#"{"schema_version":3,"grants":[],"sessions":[]}"#,
        )
        .expect("future schema should be written");
        assert!(matches!(
            load_file(schema_path.path()),
            Err(RepositoryError::UnsupportedSchema(3))
        ));

        let oversized_path = TemporaryStatePath::new("oversized");
        let file = fs::File::create(oversized_path.path()).expect("state file should be created");
        file.set_len(MAX_STATE_FILE_BYTES as u64 + 1)
            .expect("sparse state file should be enlarged");
        assert!(matches!(
            load_file(oversized_path.path()),
            Err(RepositoryError::TooLarge)
        ));
    }

    fn valid_state() -> PersistedState {
        PersistedState {
            schema_version: CURRENT_STATE_SCHEMA_VERSION,
            grants: vec![AccessGrantRecord {
                id: "Grant00001".to_owned(),
                secret_hash: VALID_HASH.to_owned(),
                label: "Valid grant".to_owned(),
                max_connections: 1,
                expires_at_epoch_seconds: Some(300),
                created_at_epoch_seconds: 100,
                paused: false,
                protocols: Default::default(),
                allowance: None,
                allowance_since_epoch_seconds: None,
                usage: None,
            }],
            sessions: vec![SessionRecord {
                id: "Session00001".to_owned(),
                grant_id: "Grant00001".to_owned(),
                secret_hash: Some(VALID_HASH.to_owned()),
                control_secret_hash: None,
                data_plane_secret_hash: None,
                resource_slot: Some(0),
                client_name: "Valid client".to_owned(),
                issued_at_epoch_seconds: 100,
                expires_at_epoch_seconds: 200,
            }],
        }
    }
}
