use std::{sync::Arc, time::Duration};

use ket_core::{
    AccessGrantSummary, CreateAccessGrantBatchRequest, CreateAccessGrantRequest,
    CreateAccessGrantResponse, SecretString, generate_access_code, generate_scoped_token,
    generate_session_token, hash_secret, hash_token_secret, split_access_code, split_session_token,
    verify_secret, verify_token_secret,
};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

use crate::{
    model::{AccessGrantRecord, PersistedState, SessionRecord},
    repository::{FileStateRepository, RepositoryError, StateRepository},
};

const MAX_LABEL_LENGTH: usize = 64;
const MAX_CLIENT_NAME_LENGTH: usize = 96;

#[derive(Clone)]
pub struct CreatedSession {
    pub token: SecretString,
    pub data_plane_token: SecretString,
    pub id: String,
    pub client_name: String,
    pub expires_at_epoch_seconds: u64,
}

impl std::fmt::Debug for CreatedSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreatedSession")
            .field("token", &"[REDACTED]")
            .field("data_plane_token", &"[REDACTED]")
            .field("id", &self.id)
            .field("client_name", &self.client_name)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct SessionView {
    pub id: String,
    pub client_name: String,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("credentials are invalid")]
    Unauthorized,
    #[error("the access grant has expired")]
    GrantExpired,
    #[error("the access grant connection limit has been reached")]
    GrantCapacity,
    #[error("the node session capacity has been reached")]
    NodeCapacity,
    #[error("the requested resource does not exist")]
    NotFound,
    #[error("secure secret processing failed")]
    Crypto,
    #[error("state persistence failed: {0}")]
    Persistence(#[from] RepositoryError),
    #[error("secure worker failed: {0}")]
    CryptoWorker(#[from] tokio::task::JoinError),
}

pub struct AccessService {
    repository: Arc<dyn StateRepository>,
    state: Mutex<PersistedState>,
    crypto_slots: Arc<Semaphore>,
    session_ttl: Duration,
    max_sessions: u32,
}

impl AccessService {
    pub async fn load_from_file(
        path: impl Into<std::path::PathBuf>,
        session_ttl: Duration,
        max_sessions: u32,
    ) -> Result<Self, ServiceError> {
        Self::load(
            Arc::new(FileStateRepository::new(path)),
            session_ttl,
            max_sessions,
        )
        .await
    }

    pub(crate) async fn load(
        repository: Arc<dyn StateRepository>,
        session_ttl: Duration,
        max_sessions: u32,
    ) -> Result<Self, ServiceError> {
        let state = repository.load().await?;
        Ok(Self {
            repository,
            state: Mutex::new(state),
            crypto_slots: Arc::new(Semaphore::new(4)),
            session_ttl,
            max_sessions,
        })
    }

    pub async fn create_grant(
        &self,
        request: CreateAccessGrantRequest,
    ) -> Result<CreateAccessGrantResponse, ServiceError> {
        let label = validate_text(request.label, "label", MAX_LABEL_LENGTH)?;
        if request.max_connections == 0 || request.max_connections > self.max_sessions {
            return Err(ServiceError::InvalidInput(format!(
                "max_connections must be between 1 and {}",
                self.max_sessions
            )));
        }
        let now = unix_time();
        if request
            .expires_at_epoch_seconds
            .is_some_and(|expires_at| expires_at <= now)
        {
            return Err(ServiceError::InvalidInput(
                "expires_at_epoch_seconds must be in the future".to_owned(),
            ));
        }

        let code = generate_access_code();
        let parts = split_access_code(&code).map_err(|_| ServiceError::Crypto)?;
        let secret_hash = self.hash(parts.secret).await?;
        let record = AccessGrantRecord {
            id: parts.id.clone(),
            secret_hash,
            label: label.clone(),
            max_connections: request.max_connections,
            expires_at_epoch_seconds: request.expires_at_epoch_seconds,
            created_at_epoch_seconds: now,
        };

        let mut state = self.state.lock().await;
        if state.grants.iter().any(|grant| grant.id == record.id) {
            return Err(ServiceError::Crypto);
        }
        let mut next = state.clone();
        next.grants.push(record);
        self.repository.store(&next).await?;
        *state = next;

        Ok(CreateAccessGrantResponse {
            id: parts.id,
            access_code: code.into(),
            label,
            max_connections: request.max_connections,
            expires_at_epoch_seconds: request.expires_at_epoch_seconds,
            created_at_epoch_seconds: now,
        })
    }

    pub async fn create_grant_batch(
        &self,
        request: CreateAccessGrantBatchRequest,
    ) -> Result<Vec<CreateAccessGrantResponse>, ServiceError> {
        if request.count == 0 || request.count > 100 {
            return Err(ServiceError::InvalidInput(
                "count must be between 1 and 100".to_owned(),
            ));
        }
        let label_prefix = validate_text(request.label_prefix, "label_prefix", MAX_LABEL_LENGTH)?;
        let mut grants = Vec::with_capacity(request.count as usize);
        for index in 1..=request.count {
            grants.push(
                self.create_grant(CreateAccessGrantRequest {
                    label: format!("{label_prefix}-{index}"),
                    max_connections: request.max_connections,
                    expires_at_epoch_seconds: request.expires_at_epoch_seconds,
                })
                .await?,
            );
        }
        Ok(grants)
    }

    pub async fn list_grants(&self) -> Vec<AccessGrantSummary> {
        let now = unix_time();
        let state = self.state.lock().await;
        state
            .grants
            .iter()
            .map(|grant| AccessGrantSummary {
                id: grant.id.clone(),
                label: grant.label.clone(),
                max_connections: grant.max_connections,
                active_connections: state
                    .sessions
                    .iter()
                    .filter(|session| {
                        session.grant_id == grant.id && session.expires_at_epoch_seconds > now
                    })
                    .count() as u32,
                expires_at_epoch_seconds: grant.expires_at_epoch_seconds,
                created_at_epoch_seconds: grant.created_at_epoch_seconds,
            })
            .collect()
    }

    pub async fn revoke_grant(&self, id: &str) -> Result<Vec<String>, ServiceError> {
        let mut state = self.state.lock().await;
        if !state.grants.iter().any(|grant| grant.id == id) {
            return Err(ServiceError::NotFound);
        }
        let mut next = state.clone();
        let revoked_sessions = next
            .sessions
            .iter()
            .filter(|session| session.grant_id == id)
            .map(|session| session.id.clone())
            .collect();
        next.grants.retain(|grant| grant.id != id);
        next.sessions.retain(|session| session.grant_id != id);
        self.repository.store(&next).await?;
        *state = next;
        Ok(revoked_sessions)
    }

    pub async fn create_session(
        &self,
        access_code: &str,
        client_name: String,
    ) -> Result<CreatedSession, ServiceError> {
        let client_name = validate_text(client_name, "client_name", MAX_CLIENT_NAME_LENGTH)?;
        let code = split_access_code(access_code).map_err(|_| ServiceError::Unauthorized)?;
        let grant = {
            let state = self.state.lock().await;
            state
                .grants
                .iter()
                .find(|grant| grant.id == code.id)
                .cloned()
        }
        .ok_or(ServiceError::Unauthorized)?;
        if !self.verify(code.secret, grant.secret_hash).await? {
            return Err(ServiceError::Unauthorized);
        }

        let now = unix_time();
        let token = generate_session_token();
        let token_parts = split_session_token(&token).map_err(|_| ServiceError::Crypto)?;
        let secret_hash = self.hash(token_parts.secret).await?;
        let data_plane_token =
            generate_scoped_token(&token_parts.id).map_err(|_| ServiceError::Crypto)?;
        let data_plane_parts =
            split_session_token(&data_plane_token).map_err(|_| ServiceError::Crypto)?;
        let data_plane_secret_hash = hash_token_secret(&data_plane_parts.secret);

        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let grant = next
            .grants
            .iter()
            .find(|candidate| candidate.id == code.id)
            .ok_or(ServiceError::Unauthorized)?;
        if grant
            .expires_at_epoch_seconds
            .is_some_and(|expires_at| expires_at <= now)
        {
            return Err(ServiceError::GrantExpired);
        }
        let active_for_grant = next
            .sessions
            .iter()
            .filter(|session| {
                session.grant_id == grant.id && session.expires_at_epoch_seconds > now
            })
            .count() as u32;
        if active_for_grant >= grant.max_connections {
            return Err(ServiceError::GrantCapacity);
        }
        let active_sessions = next
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds > now)
            .count() as u32;
        if active_sessions >= self.max_sessions {
            return Err(ServiceError::NodeCapacity);
        }

        let mut expires_at = now.saturating_add(self.session_ttl.as_secs());
        if let Some(grant_expiry) = grant.expires_at_epoch_seconds {
            expires_at = expires_at.min(grant_expiry);
        }
        let session = SessionRecord {
            id: token_parts.id.clone(),
            grant_id: grant.id.clone(),
            secret_hash,
            data_plane_secret_hash: Some(data_plane_secret_hash),
            client_name: client_name.clone(),
            issued_at_epoch_seconds: now,
            expires_at_epoch_seconds: expires_at,
        };
        if next.sessions.iter().any(|item| item.id == session.id) {
            return Err(ServiceError::Crypto);
        }
        next.sessions.push(session);
        self.repository.store(&next).await?;
        *state = next;

        Ok(CreatedSession {
            token: token.into(),
            data_plane_token: data_plane_token.into(),
            id: token_parts.id,
            client_name,
            expires_at_epoch_seconds: expires_at,
        })
    }

    pub async fn session(&self, token: &str) -> Result<SessionView, ServiceError> {
        let (record, _) = self.authenticate(token).await?;
        Ok(SessionView {
            id: record.id,
            client_name: record.client_name,
            expires_at_epoch_seconds: record.expires_at_epoch_seconds,
        })
    }

    pub async fn authenticate_data_plane(&self, token: &str) -> Result<SessionView, ServiceError> {
        let parts = split_session_token(token).map_err(|_| ServiceError::Unauthorized)?;
        let (session, grant) = {
            let state = self.state.lock().await;
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == parts.id)
                .cloned()
                .ok_or(ServiceError::Unauthorized)?;
            let grant = state
                .grants
                .iter()
                .find(|grant| grant.id == session.grant_id)
                .cloned()
                .ok_or(ServiceError::Unauthorized)?;
            (session, grant)
        };
        let expected_hash = session
            .data_plane_secret_hash
            .as_ref()
            .ok_or(ServiceError::Unauthorized)?;
        if !verify_token_secret(&parts.secret, expected_hash) {
            return Err(ServiceError::Unauthorized);
        }
        let now = unix_time();
        if session.expires_at_epoch_seconds <= now
            || grant
                .expires_at_epoch_seconds
                .is_some_and(|expiry| expiry <= now)
        {
            return Err(ServiceError::Unauthorized);
        }
        Ok(SessionView {
            id: session.id,
            client_name: session.client_name,
            expires_at_epoch_seconds: session.expires_at_epoch_seconds,
        })
    }

    pub async fn renew_session(&self, token: &str) -> Result<SessionView, ServiceError> {
        let (record, grant) = self.authenticate(token).await?;
        let now = unix_time();
        let mut expires_at = now.saturating_add(self.session_ttl.as_secs());
        if let Some(grant_expiry) = grant.expires_at_epoch_seconds {
            expires_at = expires_at.min(grant_expiry);
        }

        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let session = next
            .sessions
            .iter_mut()
            .find(|session| session.id == record.id)
            .ok_or(ServiceError::Unauthorized)?;
        session.expires_at_epoch_seconds = expires_at;
        self.repository.store(&next).await?;
        *state = next;
        Ok(SessionView {
            id: record.id,
            client_name: record.client_name,
            expires_at_epoch_seconds: expires_at,
        })
    }

    pub async fn release_session(&self, token: &str) -> Result<String, ServiceError> {
        let (record, _) = self.authenticate(token).await?;
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        next.sessions.retain(|session| session.id != record.id);
        self.repository.store(&next).await?;
        *state = next;
        Ok(record.id)
    }

    pub async fn expire_sessions(&self) -> Result<Vec<String>, ServiceError> {
        let now = unix_time();
        let mut state = self.state.lock().await;
        let expired: Vec<_> = state
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds <= now)
            .map(|session| session.id.clone())
            .collect();
        if expired.is_empty() {
            return Ok(expired);
        }
        let mut next = state.clone();
        next.sessions
            .retain(|session| session.expires_at_epoch_seconds > now);
        self.repository.store(&next).await?;
        *state = next;
        Ok(expired)
    }

    pub async fn active_session_count(&self) -> u32 {
        let now = unix_time();
        self.state
            .lock()
            .await
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds > now)
            .count() as u32
    }

    pub async fn active_session_ids(&self) -> Vec<String> {
        let now = unix_time();
        self.state
            .lock()
            .await
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds > now)
            .map(|session| session.id.clone())
            .collect()
    }

    async fn authenticate(
        &self,
        token: &str,
    ) -> Result<(SessionRecord, AccessGrantRecord), ServiceError> {
        let parts = split_session_token(token).map_err(|_| ServiceError::Unauthorized)?;
        let (session, grant) = {
            let state = self.state.lock().await;
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == parts.id)
                .cloned()
                .ok_or(ServiceError::Unauthorized)?;
            let grant = state
                .grants
                .iter()
                .find(|grant| grant.id == session.grant_id)
                .cloned()
                .ok_or(ServiceError::Unauthorized)?;
            (session, grant)
        };
        if !self
            .verify(parts.secret, session.secret_hash.clone())
            .await?
        {
            return Err(ServiceError::Unauthorized);
        }
        let now = unix_time();
        if session.expires_at_epoch_seconds <= now
            || grant
                .expires_at_epoch_seconds
                .is_some_and(|expiry| expiry <= now)
        {
            return Err(ServiceError::Unauthorized);
        }
        Ok((session, grant))
    }

    async fn hash(&self, secret: String) -> Result<String, ServiceError> {
        let permit = self
            .crypto_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ServiceError::Crypto)?;
        let result = tokio::task::spawn_blocking(move || hash_secret(&secret)).await?;
        drop(permit);
        result.map_err(|_| ServiceError::Crypto)
    }

    async fn verify(&self, secret: String, hash: String) -> Result<bool, ServiceError> {
        let permit = self
            .crypto_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ServiceError::Crypto)?;
        let result = tokio::task::spawn_blocking(move || verify_secret(&secret, &hash)).await?;
        drop(permit);
        Ok(result)
    }
}

fn validate_text(value: String, field: &str, maximum: usize) -> Result<String, ServiceError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(ServiceError::InvalidInput(format!(
            "{field} must contain between 1 and {maximum} printable characters"
        )));
    }
    Ok(value)
}

pub fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
