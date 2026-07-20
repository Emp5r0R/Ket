use std::collections::{HashMap, HashSet};

use ket_core::is_supported_secret_hash;
use serde::{Deserialize, Serialize};

const ACCESS_GRANT_ID_LENGTH: usize = 10;
const SESSION_ID_LENGTH: usize = 12;
const MAX_SECRET_HASH_LENGTH: usize = 512;
const MAX_LABEL_LENGTH: usize = 64;
const MAX_CLIENT_NAME_LENGTH: usize = 96;

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

impl PersistedState {
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut grants_by_id = HashMap::with_capacity(self.grants.len());
        for grant in &self.grants {
            if !valid_identifier(&grant.id, ACCESS_GRANT_ID_LENGTH) {
                return Err("access grant ID is invalid");
            }
            if grants_by_id
                .insert(grant.id.as_str(), grant.expires_at_epoch_seconds)
                .is_some()
            {
                return Err("access grant IDs are duplicated");
            }
            if !valid_text(&grant.secret_hash, MAX_SECRET_HASH_LENGTH) {
                return Err("access grant hash is invalid");
            }
            if !is_supported_secret_hash(&grant.secret_hash) {
                return Err("access grant hash profile is unsupported");
            }
            if !valid_text(&grant.label, MAX_LABEL_LENGTH) {
                return Err("access grant label is invalid");
            }
            if grant.max_connections == 0 {
                return Err("access grant connection limit is invalid");
            }
            if grant
                .expires_at_epoch_seconds
                .is_some_and(|expires_at| expires_at <= grant.created_at_epoch_seconds)
            {
                return Err("access grant expiry is invalid");
            }
        }

        let mut session_ids = HashSet::with_capacity(self.sessions.len());
        for session in &self.sessions {
            if !valid_identifier(&session.id, SESSION_ID_LENGTH) {
                return Err("session ID is invalid");
            }
            if !session_ids.insert(session.id.as_str()) {
                return Err("session IDs are duplicated");
            }
            let Some(grant_expiry) = grants_by_id.get(session.grant_id.as_str()) else {
                return Err("session references an unknown access grant");
            };
            if !valid_text(&session.secret_hash, MAX_SECRET_HASH_LENGTH) {
                return Err("session hash is invalid");
            }
            if !is_supported_secret_hash(&session.secret_hash) {
                return Err("session hash profile is unsupported");
            }
            if !valid_text(&session.client_name, MAX_CLIENT_NAME_LENGTH) {
                return Err("session client name is invalid");
            }
            if session.expires_at_epoch_seconds <= session.issued_at_epoch_seconds {
                return Err("session expiry is invalid");
            }
            if grant_expiry
                .is_some_and(|grant_expiry| session.expires_at_epoch_seconds > grant_expiry)
            {
                return Err("session outlives its access grant");
            }
        }
        Ok(())
    }
}

fn valid_identifier(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value == value.trim()
        && !value.chars().any(char::is_control)
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

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn malformed_ids_and_impossible_lifetimes_fail_closed() {
        let mut state = valid_state();
        state.grants[0].id = "short".to_owned();
        assert_eq!(state.validate(), Err("access grant ID is invalid"));

        let mut state = valid_state();
        state.grants[0].max_connections = 0;
        assert_eq!(
            state.validate(),
            Err("access grant connection limit is invalid")
        );

        let mut state = valid_state();
        state.grants[0].expires_at_epoch_seconds = Some(100);
        assert_eq!(state.validate(), Err("access grant expiry is invalid"));

        let mut state = valid_state();
        state.sessions[0].id = "short".to_owned();
        assert_eq!(state.validate(), Err("session ID is invalid"));

        let mut state = valid_state();
        state.sessions[0].expires_at_epoch_seconds = 100;
        assert_eq!(state.validate(), Err("session expiry is invalid"));

        let mut state = valid_state();
        state.sessions[0].expires_at_epoch_seconds = 301;
        assert_eq!(state.validate(), Err("session outlives its access grant"));
    }

    #[test]
    fn unbounded_persisted_hash_profiles_fail_before_verification() {
        let mut state = valid_state();
        state.grants[0].secret_hash = VALID_HASH.replacen("m=19456", "m=4294967295", 1);

        assert_eq!(
            state.validate(),
            Err("access grant hash profile is unsupported")
        );
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
                client_name: "Valid client".to_owned(),
                issued_at_epoch_seconds: 100,
                expires_at_epoch_seconds: 200,
            }],
        }
    }
}
