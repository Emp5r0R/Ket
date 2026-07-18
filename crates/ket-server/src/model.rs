use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistedState {
    pub schema_version: u32,
    pub grants: Vec<AccessGrantRecord>,
    pub sessions: Vec<SessionRecord>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            grants: Vec::new(),
            sessions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AccessGrantRecord {
    pub id: String,
    pub secret_hash: String,
    pub label: String,
    pub max_connections: u32,
    pub expires_at_epoch_seconds: Option<u64>,
    pub created_at_epoch_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SessionRecord {
    pub id: String,
    pub grant_id: String,
    pub secret_hash: String,
    #[serde(default)]
    pub data_plane_secret_hash: Option<[u8; 32]>,
    pub client_name: String,
    pub issued_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
}
