use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use thiserror::Error;

use crate::model::PersistedState;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("state I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("state data is invalid: {0}")]
    InvalidData(#[from] serde_json::Error),
    #[error("unsupported state schema version {0}")]
    UnsupportedSchema(u32),
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
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersistedState::default());
        }
        Err(error) => return Err(error.into()),
    };
    let state: PersistedState = serde_json::from_slice(&content)?;
    if state.schema_version != 1 {
        return Err(RepositoryError::UnsupportedSchema(state.schema_version));
    }
    Ok(state)
}

fn store_file(path: &Path, state: &PersistedState) -> Result<(), RepositoryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary_path = path.with_extension("json.tmp");
    let encoded = serde_json::to_vec_pretty(state)?;

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
