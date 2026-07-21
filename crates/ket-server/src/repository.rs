use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use thiserror::Error;

use crate::model::PersistedState;

const MAX_STATE_FILE_BYTES: usize = 64 * 1024 * 1024;

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
    let state: PersistedState = serde_json::from_slice(&content)?;
    if state.schema_version != 1 {
        return Err(RepositoryError::UnsupportedSchema(state.schema_version));
    }
    state.validate().map_err(RepositoryError::InvalidState)?;
    Ok(state)
}

fn store_file(path: &Path, state: &PersistedState) -> Result<(), RepositoryError> {
    if state.schema_version != 1 {
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

        assert_eq!(state.grants.len(), 1);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].data_plane_secret_hash, None);
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
            br#"{"schema_version":2,"grants":[],"sessions":[]}"#,
        )
        .expect("future schema should be written");
        assert!(matches!(
            load_file(schema_path.path()),
            Err(RepositoryError::UnsupportedSchema(2))
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
            schema_version: 1,
            grants: vec![AccessGrantRecord {
                id: "Grant00001".to_owned(),
                secret_hash: VALID_HASH.to_owned(),
                label: "Valid grant".to_owned(),
                max_connections: 1,
                expires_at_epoch_seconds: Some(300),
                created_at_epoch_seconds: 100,
            }],
            sessions: vec![SessionRecord {
                id: "Session00001".to_owned(),
                grant_id: "Grant00001".to_owned(),
                secret_hash: VALID_HASH.to_owned(),
                data_plane_secret_hash: None,
                resource_slot: Some(0),
                client_name: "Valid client".to_owned(),
                issued_at_epoch_seconds: 100,
                expires_at_epoch_seconds: 200,
            }],
        }
    }
}
