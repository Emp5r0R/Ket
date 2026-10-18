use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ket_core::{
    AccessGrantSummary, AllowanceStatus, CreateAccessGrantBatchRequest, CreateAccessGrantRequest,
    CreateAccessGrantResponse, DataAllowance, GrantActivity, GrantActivityEvent,
    MAX_ACCESS_GRANT_VALIDITY_MINUTES, ProtocolAccess, SecretString, TransportProtocol,
    UpdateAccessGrantRequest, generate_access_code, generate_scoped_token, generate_session_token,
    hash_secret, hash_token_secret, split_access_code, split_session_token, verify_secret,
    verify_token_secret,
};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

use crate::{
    activity::{Activity, ActivityLog},
    allowance, grant_admin,
    meter::MeteredSession,
    model::{AccessGrantRecord, PersistedState, SessionRecord},
    repository::{FileStateRepository, RepositoryError, StateRepository},
};

const MAX_LABEL_LENGTH: usize = 64;
const MAX_CLIENT_NAME_LENGTH: usize = 96;
const CRYPTO_WORKERS: usize = 4;
const MAX_PENDING_CRYPTO_OPERATIONS: usize = 32;
/// How long an expired invite stays listed before the server forgets it, so
/// operators can still see why a device stopped connecting.
const EXPIRED_GRANT_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
/// How often counted data is written to the state file when nothing else
/// writes it. Ket restarting loses at most this much counting.
const USAGE_STORE_INTERVAL_SECONDS: u64 = 60;

#[derive(Clone)]
pub struct CreatedSession {
    pub token: SecretString,
    pub data_plane_token: SecretString,
    pub id: String,
    pub grant_id: String,
    pub resource_slot: u32,
    pub expired_allocations: Vec<SessionAllocation>,
    pub client_name: String,
    pub expires_at_epoch_seconds: u64,
    pub access_expires_at_epoch_seconds: Option<u64>,
    pub protocols: ProtocolAccess,
    pub allowance: Option<AllowanceStatus>,
}

impl std::fmt::Debug for CreatedSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreatedSession")
            .field("token", &"[REDACTED]")
            .field("data_plane_token", &"[REDACTED]")
            .field("id", &self.id)
            .field("grant_id", &self.grant_id)
            .field("resource_slot", &self.resource_slot)
            .field("expired_allocations", &self.expired_allocations.len())
            .field("client_name", &self.client_name)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .field(
                "access_expires_at_epoch_seconds",
                &self.access_expires_at_epoch_seconds,
            )
            .field("protocols", &self.protocols)
            .field("allowance", &self.allowance)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct SessionView {
    pub id: String,
    pub grant_id: String,
    pub resource_slot: u32,
    pub client_name: String,
    pub expires_at_epoch_seconds: u64,
    pub access_expires_at_epoch_seconds: Option<u64>,
    /// The protocols the session's access grant allows.
    pub protocols: ProtocolAccess,
    pub allowance: Option<AllowanceStatus>,
}

/// A device connected through an access grant.
#[derive(Clone, Debug)]
pub struct GrantDevice {
    pub allocation: SessionAllocation,
    pub client_name: String,
    pub connected_at_epoch_seconds: u64,
    pub session_expires_at_epoch_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionAllocation {
    pub id: String,
    pub resource_slot: u32,
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
    #[error("the server owner has paused this invite")]
    GrantPaused,
    #[error(
        "this invite has used its data allowance; it works again from {}",
        ket_core::utc_minute(*resets_at_epoch_seconds)
    )]
    AllowanceUsed { resets_at_epoch_seconds: u64 },
    #[error("the node session capacity has been reached")]
    NodeCapacity,
    #[error("secure secret processing is temporarily busy")]
    Busy,
    #[error("the requested resource does not exist")]
    NotFound,
    /// Valid credentials for something the grant does not allow.
    #[error("the access grant does not allow this")]
    Forbidden,
    #[error("secure secret processing failed")]
    Crypto,
    #[error("state persistence failed: {0}")]
    Persistence(#[from] RepositoryError),
    #[error("persistent session state is invalid: {0}")]
    InvalidState(&'static str),
    #[error("secure worker failed: {0}")]
    CryptoWorker(#[from] tokio::task::JoinError),
}

pub struct AccessService {
    repository: Arc<dyn StateRepository>,
    state: Mutex<PersistedState>,
    crypto_slots: Arc<Semaphore>,
    crypto_admission: Arc<Semaphore>,
    crypto_overloads: AtomicU64,
    session_ttl: Duration,
    max_sessions: u32,
    /// In memory only; see [`crate::activity`].
    activity: std::sync::Mutex<ActivityLog>,
    /// When counted data was last written to the state file.
    usage_stored_at: AtomicU64,
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
        let mut state = repository.load().await?;
        let changed = prepare_resource_slots(&mut state, max_sessions)?;
        if changed {
            repository.store(&state).await?;
        }
        Ok(Self {
            repository,
            state: Mutex::new(state),
            crypto_slots: Arc::new(Semaphore::new(CRYPTO_WORKERS)),
            crypto_admission: Arc::new(Semaphore::new(MAX_PENDING_CRYPTO_OPERATIONS)),
            crypto_overloads: AtomicU64::new(0),
            session_ttl,
            max_sessions,
            activity: std::sync::Mutex::default(),
            usage_stored_at: AtomicU64::new(unix_time()),
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
        if request.valid_for_minutes == 0
            || request.valid_for_minutes > MAX_ACCESS_GRANT_VALIDITY_MINUTES
        {
            return Err(ServiceError::InvalidInput(format!(
                "valid_for_minutes must be between 1 and {MAX_ACCESS_GRANT_VALIDITY_MINUTES}"
            )));
        }
        validate_protocols(&request.protocols)?;
        validate_allowance(request.allowance.as_ref())?;
        let now = unix_time();
        let expires_at_epoch_seconds =
            now.saturating_add(u64::from(request.valid_for_minutes) * 60);

        let code = generate_access_code();
        let parts = split_access_code(&code).map_err(|_| ServiceError::Crypto)?;
        let secret_hash = self.hash(parts.secret).await?;
        let record = AccessGrantRecord {
            id: parts.id.clone(),
            secret_hash,
            label: label.clone(),
            max_connections: request.max_connections,
            expires_at_epoch_seconds: Some(expires_at_epoch_seconds),
            created_at_epoch_seconds: now,
            paused: false,
            protocols: request.protocols.clone(),
            allowance: request.allowance,
            allowance_since_epoch_seconds: request.allowance.map(|_| now),
            usage: None,
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
            valid_for_minutes: request.valid_for_minutes,
            expires_at_epoch_seconds,
            created_at_epoch_seconds: now,
            protocols: request.protocols,
            allowance: request.allowance,
            invite: None,
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
                    valid_for_minutes: request.valid_for_minutes,
                    protocols: request.protocols.clone(),
                    allowance: request.allowance,
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
            .map(|grant| grant_summary(&state, grant, now))
            .collect()
    }

    /// One grant and the devices connected through it, oldest first.
    pub async fn grant_details(
        &self,
        id: &str,
    ) -> Result<(AccessGrantSummary, Vec<GrantDevice>), ServiceError> {
        let now = unix_time();
        let state = self.state.lock().await;
        let grant = state
            .grants
            .iter()
            .find(|grant| grant.id == id)
            .ok_or(ServiceError::NotFound)?;
        let mut devices = state
            .sessions
            .iter()
            .filter(|session| session.grant_id == id && session.expires_at_epoch_seconds > now)
            .map(|session| {
                Ok(GrantDevice {
                    allocation: session_allocation(session)?,
                    client_name: session.client_name.clone(),
                    connected_at_epoch_seconds: session.issued_at_epoch_seconds,
                    session_expires_at_epoch_seconds: session.expires_at_epoch_seconds,
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        devices.sort_by_key(|device| device.connected_at_epoch_seconds);
        Ok((grant_summary(&state, grant, now), devices))
    }

    /// Changes a grant and returns it with the sessions the change ended,
    /// which the caller must disconnect from the data plane.
    pub async fn update_grant(
        &self,
        id: &str,
        mut changes: UpdateAccessGrantRequest,
    ) -> Result<(AccessGrantSummary, Vec<SessionAllocation>), ServiceError> {
        if changes.is_empty() {
            return Err(ServiceError::InvalidInput(
                "the request changes nothing".to_owned(),
            ));
        }
        if let Some(label) = changes.label.take() {
            changes.label = Some(validate_text(label, "label", MAX_LABEL_LENGTH)?);
        }
        if changes
            .max_connections
            .is_some_and(|devices| devices == 0 || devices > self.max_sessions)
        {
            return Err(ServiceError::InvalidInput(format!(
                "max_connections must be between 1 and {}",
                self.max_sessions
            )));
        }
        let now = unix_time();
        let latest_end = now.saturating_add(u64::from(MAX_ACCESS_GRANT_VALIDITY_MINUTES) * 60);
        if changes
            .expires_at_epoch_seconds
            .is_some_and(|expires_at| expires_at <= now || expires_at > latest_end)
        {
            return Err(ServiceError::InvalidInput(
                "expires_at_epoch_seconds must be in the future and at most 365 days away"
                    .to_owned(),
            ));
        }
        if let Some(protocols) = &changes.protocols {
            validate_protocols(protocols)?;
        }
        if let Some(allowance) = &changes.allowance {
            validate_allowance(allowance.as_ref())?;
        }

        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let ended = grant_admin::apply(&mut next, id, &changes, now)?;
        self.repository.store(&next).await?;
        let ended_sessions = state
            .sessions
            .iter()
            .filter(|session| ended.iter().any(|allocation| allocation.id == session.id))
            .cloned()
            .collect::<Vec<_>>();
        *state = next;
        self.note_ended(&state, &ended_sessions, GrantActivityEvent::EndedByChange);
        let grant = state
            .grants
            .iter()
            .find(|grant| grant.id == id)
            .ok_or(ServiceError::NotFound)?;
        Ok((grant_summary(&state, grant, now), ended))
    }

    pub async fn revoke_grant(&self, id: &str) -> Result<Vec<SessionAllocation>, ServiceError> {
        let mut state = self.state.lock().await;
        if !state.grants.iter().any(|grant| grant.id == id) {
            return Err(ServiceError::NotFound);
        }
        let mut next = state.clone();
        let revoked_sessions = next
            .sessions
            .iter()
            .filter(|session| session.grant_id == id)
            .map(session_allocation)
            .collect::<Result<Vec<_>, _>>()?;
        next.grants.retain(|grant| grant.id != id);
        next.sessions.retain(|session| session.grant_id != id);
        self.repository.store(&next).await?;
        *state = next;
        self.forget_missing_activity(&state);
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
        let result = self
            .admit_session(grant, code.secret, client_name.clone())
            .await;
        if let Err(error) = &result
            && let Some(event) = enrollment_refusal(error)
        {
            let state = self.state.lock().await;
            self.note(&state, &code.id, Activity::enrollment(event, &client_name));
        }
        result
    }

    async fn admit_session(
        &self,
        grant: AccessGrantRecord,
        secret: String,
        client_name: String,
    ) -> Result<CreatedSession, ServiceError> {
        if !self.verify(secret, grant.secret_hash.clone()).await? {
            return Err(ServiceError::Unauthorized);
        }
        if grant.paused {
            return Err(ServiceError::GrantPaused);
        }
        let now = unix_time();
        if let Some(resets_at_epoch_seconds) = allowance::used_up_until(&grant, now) {
            return Err(ServiceError::AllowanceUsed {
                resets_at_epoch_seconds,
            });
        }

        let token = generate_session_token();
        let token_parts = split_session_token(&token).map_err(|_| ServiceError::Crypto)?;
        let control_secret_hash = hash_token_secret(&token_parts.secret);
        let data_plane_token =
            generate_scoped_token(&token_parts.id).map_err(|_| ServiceError::Crypto)?;
        let data_plane_parts =
            split_session_token(&data_plane_token).map_err(|_| ServiceError::Crypto)?;
        let data_plane_secret_hash = hash_token_secret(&data_plane_parts.secret);

        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let expired = take_expired_sessions(&mut next, now);
        let expired_allocations = expired
            .iter()
            .map(session_allocation)
            .collect::<Result<Vec<_>, _>>()?;
        let grant = next
            .grants
            .iter()
            .find(|candidate| candidate.id == grant.id)
            .ok_or(ServiceError::Unauthorized)?;
        if grant
            .expires_at_epoch_seconds
            .is_some_and(|expires_at| expires_at <= now)
        {
            return Err(ServiceError::GrantExpired);
        }
        if grant.paused {
            return Err(ServiceError::GrantPaused);
        }
        if let Some(resets_at_epoch_seconds) = allowance::used_up_until(grant, now) {
            return Err(ServiceError::AllowanceUsed {
                resets_at_epoch_seconds,
            });
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
        let resource_slot =
            next_resource_slot(&next, self.max_sessions).ok_or(ServiceError::NodeCapacity)?;

        let access_expires_at_epoch_seconds = grant.expires_at_epoch_seconds;
        let protocols = grant.protocols.clone();
        let allowance = allowance::status(grant, now);
        let mut expires_at = now.saturating_add(self.session_ttl.as_secs());
        if let Some(grant_expiry) = access_expires_at_epoch_seconds {
            expires_at = expires_at.min(grant_expiry);
        }
        let session = SessionRecord {
            id: token_parts.id.clone(),
            grant_id: grant.id.clone(),
            secret_hash: None,
            control_secret_hash: Some(control_secret_hash),
            data_plane_secret_hash: Some(data_plane_secret_hash),
            resource_slot: Some(resource_slot),
            client_name: client_name.clone(),
            issued_at_epoch_seconds: now,
            expires_at_epoch_seconds: expires_at,
        };
        if next.sessions.iter().any(|item| item.id == session.id) {
            return Err(ServiceError::Crypto);
        }
        let grant_id = grant.id.clone();
        next.sessions.push(session);
        self.repository.store(&next).await?;
        *state = next;
        self.note_ended(&state, &expired, GrantActivityEvent::LeaseEnded);

        Ok(CreatedSession {
            token: token.into(),
            data_plane_token: data_plane_token.into(),
            id: token_parts.id,
            grant_id,
            resource_slot,
            expired_allocations,
            client_name,
            expires_at_epoch_seconds: expires_at,
            access_expires_at_epoch_seconds,
            protocols,
            allowance,
        })
    }

    pub async fn session(&self, token: &str) -> Result<SessionView, ServiceError> {
        let (record, grant) = self.authenticate(token).await?;
        let resource_slot = required_resource_slot(&record)?;
        Ok(SessionView {
            id: record.id,
            grant_id: grant.id.clone(),
            resource_slot,
            client_name: record.client_name,
            expires_at_epoch_seconds: record.expires_at_epoch_seconds,
            access_expires_at_epoch_seconds: grant.expires_at_epoch_seconds,
            allowance: allowance::status(&grant, unix_time()),
            protocols: grant.protocols,
        })
    }

    /// Signs a device in to `protocol`'s server with its data-plane token.
    /// OpenVPN also names the session as `username`. The outcome is recorded
    /// against the device's grant.
    pub async fn authenticate_data_plane(
        &self,
        token: &str,
        protocol: &TransportProtocol,
        username: Option<&str>,
    ) -> Result<SessionView, ServiceError> {
        let parts = split_session_token(token).map_err(|_| ServiceError::Unauthorized)?;
        let result = self.verify_data_plane(&parts, protocol, username).await;
        let event = match &result {
            Ok(_) => GrantActivityEvent::SignedIn,
            Err(ServiceError::Forbidden) => GrantActivityEvent::SignInRefusedProtocol,
            Err(_) => GrantActivityEvent::SignInRefused,
        };
        let state = self.state.lock().await;
        if let Some(session) = state.sessions.iter().find(|session| session.id == parts.id) {
            self.note(
                &state,
                &session.grant_id,
                Activity::session(event, &session.id, &session.client_name).over(protocol.clone()),
            );
        }
        result.map_err(|_| ServiceError::Unauthorized)
    }

    async fn verify_data_plane(
        &self,
        parts: &ket_core::SessionTokenParts,
        protocol: &TransportProtocol,
        username: Option<&str>,
    ) -> Result<SessionView, ServiceError> {
        if username.is_some_and(|username| username != parts.id) {
            return Err(ServiceError::Unauthorized);
        }
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
        if !admits(&session, &grant, now) {
            return Err(ServiceError::Unauthorized);
        }
        if !grant.protocols.allows(protocol) {
            return Err(ServiceError::Forbidden);
        }
        let resource_slot = required_resource_slot(&session)?;
        Ok(SessionView {
            id: session.id,
            allowance: allowance::status(&grant, now),
            grant_id: grant.id,
            resource_slot,
            client_name: session.client_name,
            expires_at_epoch_seconds: session.expires_at_epoch_seconds,
            access_expires_at_epoch_seconds: grant.expires_at_epoch_seconds,
            protocols: grant.protocols,
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
        self.note(
            &state,
            &grant.id,
            Activity::session(
                GrantActivityEvent::CheckedIn,
                &record.id,
                &record.client_name,
            ),
        );
        let resource_slot = required_resource_slot(&record)?;
        Ok(SessionView {
            id: record.id,
            allowance: allowance::status(&grant, now),
            grant_id: grant.id,
            resource_slot,
            client_name: record.client_name,
            expires_at_epoch_seconds: expires_at,
            access_expires_at_epoch_seconds: grant.expires_at_epoch_seconds,
            protocols: grant.protocols,
        })
    }

    /// Ends the session at the app's request.
    pub async fn release_session(&self, token: &str) -> Result<SessionAllocation, ServiceError> {
        self.end_session(token, Some(GrantActivityEvent::Left))
            .await
    }

    /// Ends a session that was created but must not be used. Its refusal is
    /// recorded by whoever refused it.
    pub(crate) async fn discard_session(
        &self,
        token: &str,
    ) -> Result<SessionAllocation, ServiceError> {
        self.end_session(token, None).await
    }

    async fn end_session(
        &self,
        token: &str,
        event: Option<GrantActivityEvent>,
    ) -> Result<SessionAllocation, ServiceError> {
        let (record, _) = self.authenticate(token).await?;
        let allocation = session_allocation(&record)?;
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        next.sessions.retain(|session| session.id != record.id);
        self.repository.store(&next).await?;
        *state = next;
        if let Some(event) = event {
            self.note_ended(&state, std::slice::from_ref(&record), event);
        }
        Ok(allocation)
    }

    /// Records `activity` against the grant, which must still exist.
    pub(crate) async fn record_activity(&self, grant_id: &str, activity: Activity) {
        let state = self.state.lock().await;
        self.note(&state, grant_id, activity);
    }

    /// What devices recently did with the grant, newest first.
    pub fn recent_activity(&self, grant_id: &str) -> Vec<GrantActivity> {
        self.activity
            .lock()
            .map(|log| log.recent(grant_id))
            .unwrap_or_default()
    }

    /// Records `activity` if `state` still holds the grant, so the log never
    /// keeps entries for grants that are gone.
    fn note(&self, state: &PersistedState, grant_id: &str, activity: Activity) {
        if !state.grants.iter().any(|grant| grant.id == grant_id) {
            return;
        }
        if let Ok(mut log) = self.activity.lock() {
            log.record(grant_id, activity, unix_time());
        }
    }

    fn note_ended(
        &self,
        state: &PersistedState,
        sessions: &[SessionRecord],
        event: GrantActivityEvent,
    ) {
        for session in sessions {
            self.note(
                state,
                &session.grant_id,
                Activity::session(event, &session.id, &session.client_name),
            );
        }
    }

    /// Drops the log of every grant `state` no longer holds.
    fn forget_missing_activity(&self, state: &PersistedState) {
        if let Ok(mut log) = self.activity.lock() {
            log.retain(|grant_id| state.grants.iter().any(|grant| grant.id == grant_id));
        }
    }

    /// Ends expired session leases and forgets invites that expired more than
    /// [`EXPIRED_GRANT_RETENTION_SECONDS`] ago, label and all.
    pub async fn expire_sessions(&self) -> Result<Vec<SessionAllocation>, ServiceError> {
        let now = unix_time();
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let ended = take_expired_sessions(&mut next, now);
        let expired = ended
            .iter()
            .map(session_allocation)
            .collect::<Result<Vec<_>, _>>()?;
        let forgot_grants = forget_stale_grants(&mut next, now);
        if expired.is_empty() && !forgot_grants {
            return Ok(expired);
        }
        self.repository.store(&next).await?;
        *state = next;
        self.note_ended(&state, &ended, GrantActivityEvent::LeaseEnded);
        self.forget_missing_activity(&state);
        Ok(expired)
    }

    /// The live sessions of every grant with a data allowance.
    pub(crate) async fn metered_sessions(&self) -> Vec<MeteredSession> {
        let now = unix_time();
        let state = self.state.lock().await;
        state
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds > now)
            .filter_map(|session| {
                let grant = state
                    .grants
                    .iter()
                    .find(|grant| grant.id == session.grant_id && grant.allowance.is_some())?;
                Some(MeteredSession {
                    allocation: session_allocation(session).ok()?,
                    issued_at_epoch_seconds: session.issued_at_epoch_seconds,
                    counted_since_epoch_seconds: grant.allowance_since_epoch_seconds.unwrap_or(0),
                })
            })
            .collect()
    }

    /// Adds what each session moved (by session ID) to its grant's total,
    /// then ends the sessions of every grant that has used up its allowance
    /// and returns them for the caller to disconnect from the data plane.
    pub(crate) async fn record_usage(
        &self,
        moved: &HashMap<String, u64>,
        now: u64,
    ) -> Result<Vec<SessionAllocation>, ServiceError> {
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let mut per_grant: HashMap<&str, u64> = HashMap::new();
        for session in &state.sessions {
            if let Some(bytes) = moved.get(&session.id).filter(|bytes| **bytes > 0) {
                let total = per_grant.entry(session.grant_id.as_str()).or_insert(0);
                *total = total.saturating_add(*bytes);
            }
        }
        for grant in &mut next.grants {
            if let Some(bytes) = per_grant.get(grant.id.as_str()) {
                allowance::add(grant, *bytes, now);
            }
        }
        let used_up = next
            .grants
            .iter()
            .filter(|grant| allowance::used_up_until(grant, now).is_some())
            .map(|grant| grant.id.clone())
            .collect::<HashSet<_>>();
        let (ending, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut next.sessions)
            .into_iter()
            .partition(|session| used_up.contains(&session.grant_id));
        next.sessions = kept;
        let ended = ending
            .iter()
            .map(session_allocation)
            .collect::<Result<Vec<_>, _>>()?;

        let stored_at = self.usage_stored_at.load(Ordering::Relaxed);
        if !ending.is_empty()
            || (!per_grant.is_empty()
                && now.saturating_sub(stored_at) >= USAGE_STORE_INTERVAL_SECONDS)
        {
            self.repository.store(&next).await?;
            self.usage_stored_at.store(now, Ordering::Relaxed);
        }
        *state = next;
        self.note_ended(&state, &ending, GrantActivityEvent::EndedAllowanceUsed);
        Ok(ended)
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

    pub async fn active_session_allocations(&self) -> Result<Vec<SessionAllocation>, ServiceError> {
        let now = unix_time();
        self.state
            .lock()
            .await
            .sessions
            .iter()
            .filter(|session| session.expires_at_epoch_seconds > now)
            .map(session_allocation)
            .collect()
    }

    pub fn crypto_operations_in_flight(&self) -> u32 {
        MAX_PENDING_CRYPTO_OPERATIONS.saturating_sub(self.crypto_admission.available_permits())
            as u32
    }

    pub fn crypto_operation_capacity(&self) -> u32 {
        MAX_PENDING_CRYPTO_OPERATIONS as u32
    }

    pub fn crypto_overload_count(&self) -> u64 {
        self.crypto_overloads.load(Ordering::Relaxed)
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
        let authorized = match (&session.control_secret_hash, &session.secret_hash) {
            (Some(expected_hash), None) => verify_token_secret(&parts.secret, expected_hash),
            (None, Some(legacy_hash)) => self.verify(parts.secret, legacy_hash.clone()).await?,
            _ => {
                return Err(ServiceError::InvalidState(
                    "session control-token hash is invalid",
                ));
            }
        };
        if !authorized {
            return Err(ServiceError::Unauthorized);
        }
        if !admits(&session, &grant, unix_time()) {
            return Err(ServiceError::Unauthorized);
        }
        Ok((session, grant))
    }

    async fn hash(&self, secret: String) -> Result<String, ServiceError> {
        let _admission = self.admit_crypto()?;
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
        let _admission = self.admit_crypto()?;
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

    fn admit_crypto(&self) -> Result<tokio::sync::OwnedSemaphorePermit, ServiceError> {
        self.crypto_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                self.crypto_overloads.fetch_add(1, Ordering::Relaxed);
                ServiceError::Busy
            })
    }
}

fn prepare_resource_slots(
    state: &mut PersistedState,
    max_sessions: u32,
) -> Result<bool, ServiceError> {
    let now = unix_time();
    let previous_len = state.sessions.len();
    state
        .sessions
        .retain(|session| session.expires_at_epoch_seconds > now);
    let mut changed = state.sessions.len() != previous_len;
    let mut used = HashSet::with_capacity(state.sessions.len());
    for session in &state.sessions {
        if let Some(slot) = session.resource_slot {
            if slot >= max_sessions {
                return Err(ServiceError::InvalidState(
                    "a session resource slot exceeds node capacity",
                ));
            }
            if !used.insert(slot) {
                return Err(ServiceError::InvalidState(
                    "session resource slots are duplicated",
                ));
            }
        }
    }
    for session in &mut state.sessions {
        if session.resource_slot.is_some() {
            continue;
        }
        let slot = (0..max_sessions).find(|slot| !used.contains(slot)).ok_or(
            ServiceError::InvalidState("active sessions exceed node resource slots"),
        )?;
        session.resource_slot = Some(slot);
        used.insert(slot);
        changed = true;
    }
    Ok(changed)
}

/// Removes and returns the sessions whose lease has run out.
fn take_expired_sessions(state: &mut PersistedState, now: u64) -> Vec<SessionRecord> {
    let (expired, live): (Vec<_>, Vec<_>) = std::mem::take(&mut state.sessions)
        .into_iter()
        .partition(|session| session.expires_at_epoch_seconds <= now);
    state.sessions = live;
    expired
}

/// The activity an enrollment refusal is recorded as, for refusals that say
/// something about the grant or the server.
fn enrollment_refusal(error: &ServiceError) -> Option<GrantActivityEvent> {
    match error {
        ServiceError::Unauthorized => Some(GrantActivityEvent::RefusedWrongCode),
        ServiceError::GrantExpired => Some(GrantActivityEvent::RefusedExpired),
        ServiceError::GrantPaused => Some(GrantActivityEvent::RefusedPaused),
        ServiceError::AllowanceUsed { .. } => Some(GrantActivityEvent::RefusedAllowanceUsed),
        ServiceError::GrantCapacity => Some(GrantActivityEvent::RefusedDeviceLimit),
        ServiceError::NodeCapacity => Some(GrantActivityEvent::RefusedServerFull),
        _ => None,
    }
}

/// Removes invites that expired at least [`EXPIRED_GRANT_RETENTION_SECONDS`]
/// ago, with any session still pointing at them. True when anything changed.
fn forget_stale_grants(state: &mut PersistedState, now: u64) -> bool {
    let stale = |grant: &AccessGrantRecord| {
        grant.expires_at_epoch_seconds.is_some_and(|expires_at| {
            now.saturating_sub(expires_at) >= EXPIRED_GRANT_RETENTION_SECONDS
        })
    };
    let before = state.grants.len();
    let forgotten: Vec<String> = state
        .grants
        .iter()
        .filter(|grant| stale(grant))
        .map(|grant| grant.id.clone())
        .collect();
    state.grants.retain(|grant| !stale(grant));
    state
        .sessions
        .retain(|session| !forgotten.contains(&session.grant_id));
    state.grants.len() != before
}

fn next_resource_slot(state: &PersistedState, max_sessions: u32) -> Option<u32> {
    let used: HashSet<_> = state
        .sessions
        .iter()
        .filter_map(|session| session.resource_slot)
        .collect();
    (0..max_sessions).find(|slot| !used.contains(slot))
}

fn required_resource_slot(session: &SessionRecord) -> Result<u32, ServiceError> {
    session.resource_slot.ok_or(ServiceError::InvalidState(
        "an active session has no resource slot",
    ))
}

pub(crate) fn session_allocation(
    session: &SessionRecord,
) -> Result<SessionAllocation, ServiceError> {
    Ok(SessionAllocation {
        id: session.id.clone(),
        resource_slot: required_resource_slot(session)?,
    })
}

fn grant_summary(
    state: &PersistedState,
    grant: &AccessGrantRecord,
    now: u64,
) -> AccessGrantSummary {
    AccessGrantSummary {
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
        paused: grant.paused,
        protocols: grant.protocols.clone(),
        allowance: allowance::status(grant, now),
    }
}

/// Whether the session's grant still lets it in: the lease and the grant
/// are current, the grant is not paused, and its allowance is not used up.
fn admits(session: &SessionRecord, grant: &AccessGrantRecord, now: u64) -> bool {
    session.expires_at_epoch_seconds > now
        && !grant.paused
        && grant
            .expires_at_epoch_seconds
            .is_none_or(|expiry| expiry > now)
        && allowance::used_up_until(grant, now).is_none()
}

fn validate_protocols(protocols: &ProtocolAccess) -> Result<(), ServiceError> {
    if protocols.is_valid() {
        Ok(())
    } else {
        Err(ServiceError::InvalidInput(
            "protocols must name at least one protocol".to_owned(),
        ))
    }
}

fn validate_allowance(allowance: Option<&DataAllowance>) -> Result<(), ServiceError> {
    if allowance.is_none_or(DataAllowance::is_valid) {
        Ok(())
    } else {
        Err(ServiceError::InvalidInput(format!(
            "allowance bytes must be between {} and {}",
            ket_core::MIN_ALLOWANCE_BYTES,
            ket_core::MAX_ALLOWANCE_BYTES
        )))
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

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyRepository;

    #[async_trait::async_trait]
    impl StateRepository for EmptyRepository {
        async fn load(&self) -> Result<PersistedState, RepositoryError> {
            Ok(PersistedState::default())
        }

        async fn store(&self, _state: &PersistedState) -> Result<(), RepositoryError> {
            Ok(())
        }
    }

    struct SnapshotRepository(PersistedState);

    #[async_trait::async_trait]
    impl StateRepository for SnapshotRepository {
        async fn load(&self) -> Result<PersistedState, RepositoryError> {
            Ok(self.0.clone())
        }

        async fn store(&self, _state: &PersistedState) -> Result<(), RepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn new_control_tokens_authenticate_while_argon_admission_is_saturated() {
        let service = AccessService::load(Arc::new(EmptyRepository), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let grant = service
            .create_grant(CreateAccessGrantRequest {
                label: "Fast session authentication".to_owned(),
                max_connections: 1,
                valid_for_minutes: 60,
                protocols: Default::default(),
                allowance: None,
            })
            .await
            .expect("grant should be created");
        let session = service
            .create_session(grant.access_code.expose_secret(), "Fast client".to_owned())
            .await
            .expect("session should be created");
        {
            let state = service.state.lock().await;
            let stored = state.sessions.first().expect("session should be stored");
            assert!(stored.secret_hash.is_none());
            assert_eq!(
                stored.control_secret_hash,
                Some(hash_token_secret(
                    &split_session_token(session.token.expose_secret())
                        .expect("generated token should split")
                        .secret,
                )),
            );
        }

        let saturated = service
            .crypto_admission
            .clone()
            .acquire_many_owned(MAX_PENDING_CRYPTO_OPERATIONS as u32)
            .await
            .expect("test should reserve the Argon admission queue");
        let current = service
            .session(session.token.expose_secret())
            .await
            .expect("fast session status should not need Argon admission");
        let renewed = service
            .renew_session(session.token.expose_secret())
            .await
            .expect("fast session renewal should not need Argon admission");

        assert_eq!(current.id, session.id);
        assert_eq!(renewed.id, session.id);
        assert!(renewed.expires_at_epoch_seconds >= current.expires_at_epoch_seconds);
        assert_eq!(service.crypto_overload_count(), 0);
        drop(saturated);
    }

    #[tokio::test]
    async fn check_ins_departures_and_lapsed_leases_are_recorded_per_device() {
        let service = AccessService::load(Arc::new(EmptyRepository), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let grant = service
            .create_grant(CreateAccessGrantRequest {
                label: "Activity".to_owned(),
                max_connections: 2,
                valid_for_minutes: 60,
                protocols: Default::default(),
                allowance: None,
            })
            .await
            .expect("grant should be created");
        let leaving = service
            .create_session(grant.access_code.expose_secret(), "linux/1.2.0".to_owned())
            .await
            .expect("session should be created");
        let lapsing = service
            .create_session(
                grant.access_code.expose_secret(),
                "android/1.2.0".to_owned(),
            )
            .await
            .expect("session should be created");
        for _ in 0..2 {
            service
                .renew_session(leaving.token.expose_secret())
                .await
                .expect("session should renew");
        }
        service
            .release_session(leaving.token.expose_secret())
            .await
            .expect("session should be released");
        // The other app stops renewing and its lease runs out.
        service.state.lock().await.sessions[0].expires_at_epoch_seconds = unix_time() - 1;
        assert_eq!(service.expire_sessions().await.unwrap().len(), 1);

        let recent = service.recent_activity(&grant.id);
        let summary = recent
            .iter()
            .map(|entry| (entry.event, entry.session_id.clone(), entry.count))
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                (GrantActivityEvent::LeaseEnded, Some(lapsing.id.clone()), 1),
                (GrantActivityEvent::Left, Some(leaving.id.clone()), 1),
                (GrantActivityEvent::CheckedIn, Some(leaving.id.clone()), 2),
            ]
        );
        assert_eq!(recent[0].client_name, "android/1.2.0");
    }

    #[tokio::test]
    async fn legacy_argon_control_tokens_still_load_authenticate_and_renew() {
        let now = unix_time();
        let session_id = "Session00001";
        let session_secret = "L".repeat(32);
        let token = format!("{session_id}{session_secret}");
        let state = PersistedState {
            schema_version: 1,
            grants: vec![AccessGrantRecord {
                id: "Grant00001".to_owned(),
                secret_hash: hash_secret("legacy-grant-secret")
                    .expect("legacy grant hash should be created"),
                label: "Legacy grant".to_owned(),
                max_connections: 1,
                expires_at_epoch_seconds: Some(now + 600),
                created_at_epoch_seconds: now.saturating_sub(60),
                paused: false,
                protocols: Default::default(),
                allowance: None,
                allowance_since_epoch_seconds: None,
                usage: None,
            }],
            sessions: vec![SessionRecord {
                id: session_id.to_owned(),
                grant_id: "Grant00001".to_owned(),
                secret_hash: Some(
                    hash_secret(&session_secret).expect("legacy session hash should be created"),
                ),
                control_secret_hash: None,
                data_plane_secret_hash: None,
                resource_slot: Some(0),
                client_name: "Legacy client".to_owned(),
                issued_at_epoch_seconds: now.saturating_sub(1),
                expires_at_epoch_seconds: now + 300,
            }],
        };
        let service = AccessService::load(
            Arc::new(SnapshotRepository(state)),
            Duration::from_secs(300),
            4,
        )
        .await
        .expect("legacy state should load");

        let current = service
            .session(&token)
            .await
            .expect("legacy session should authenticate");
        let renewed = service
            .renew_session(&token)
            .await
            .expect("legacy session should renew");

        assert_eq!(current.id, session_id);
        assert_eq!(renewed.id, session_id);
        assert!(renewed.expires_at_epoch_seconds >= current.expires_at_epoch_seconds);
    }

    #[tokio::test]
    async fn grant_duration_is_required_bounded_and_returned_as_an_absolute_expiry() {
        let service = AccessService::load(Arc::new(EmptyRepository), Duration::from_secs(300), 4)
            .await
            .expect("service should load");

        for valid_for_minutes in [0, MAX_ACCESS_GRANT_VALIDITY_MINUTES + 1] {
            let result = service
                .create_grant(CreateAccessGrantRequest {
                    label: "Invalid duration".to_owned(),
                    max_connections: 1,
                    valid_for_minutes,
                    protocols: Default::default(),
                    allowance: None,
                })
                .await;
            assert!(matches!(result, Err(ServiceError::InvalidInput(_))));
        }

        let grant = service
            .create_grant(CreateAccessGrantRequest {
                label: "One day".to_owned(),
                max_connections: 1,
                valid_for_minutes: 1_440,
                protocols: Default::default(),
                allowance: None,
            })
            .await
            .expect("bounded grant should be created");
        assert_eq!(grant.valid_for_minutes, 1_440);
        assert_eq!(
            grant.expires_at_epoch_seconds - grant.created_at_epoch_seconds,
            1_440 * 60
        );
    }

    #[tokio::test]
    async fn crypto_overload_fails_before_queuing_more_work() {
        let service = AccessService::load(Arc::new(EmptyRepository), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let saturated = service
            .crypto_admission
            .clone()
            .acquire_many_owned(MAX_PENDING_CRYPTO_OPERATIONS as u32)
            .await
            .expect("test should reserve the bounded crypto queue");
        assert_eq!(
            service.crypto_operations_in_flight(),
            service.crypto_operation_capacity()
        );

        let result = service
            .create_grant(CreateAccessGrantRequest {
                label: "Overload test".to_owned(),
                max_connections: 1,
                valid_for_minutes: 60,
                protocols: Default::default(),
                allowance: None,
            })
            .await;

        assert!(matches!(result, Err(ServiceError::Busy)));
        assert_eq!(service.crypto_overload_count(), 1);
        drop(saturated);
        assert_eq!(service.crypto_operations_in_flight(), 0);
    }

    #[test]
    fn legacy_sessions_receive_stable_slots_and_expired_slots_are_reused() {
        let now = unix_time();
        let mut state = PersistedState {
            schema_version: 1,
            grants: Vec::new(),
            sessions: vec![
                test_session("Session00001", None, now + 300),
                test_session("Session00002", Some(1), now + 300),
                test_session("Session00003", Some(0), now.saturating_sub(1)),
            ],
        };

        assert!(prepare_resource_slots(&mut state, 3).expect("migration should succeed"));
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[0].resource_slot, Some(0));
        assert_eq!(state.sessions[1].resource_slot, Some(1));
        assert!(!prepare_resource_slots(&mut state, 3).expect("migration should be stable"));

        state.sessions.remove(0);
        assert_eq!(next_resource_slot(&state, 3), Some(0));
    }

    #[test]
    fn invalid_resource_slots_fail_before_service_startup() {
        let now = unix_time();
        let mut duplicate = PersistedState {
            schema_version: 1,
            grants: Vec::new(),
            sessions: vec![
                test_session("Session00001", Some(0), now + 300),
                test_session("Session00002", Some(0), now + 300),
            ],
        };
        assert!(matches!(
            prepare_resource_slots(&mut duplicate, 2),
            Err(ServiceError::InvalidState(
                "session resource slots are duplicated"
            ))
        ));

        duplicate.sessions[1].resource_slot = Some(2);
        assert!(matches!(
            prepare_resource_slots(&mut duplicate, 2),
            Err(ServiceError::InvalidState(
                "a session resource slot exceeds node capacity"
            ))
        ));
    }

    #[test]
    fn invites_are_forgotten_a_week_after_they_expire() {
        let now = 10_000_000;
        let grant = |id: &str, expires_at: Option<u64>| AccessGrantRecord {
            id: id.to_owned(),
            secret_hash: String::new(),
            label: "Label".to_owned(),
            max_connections: 1,
            expires_at_epoch_seconds: expires_at,
            created_at_epoch_seconds: 1,
            paused: false,
            protocols: Default::default(),
            allowance: None,
            allowance_since_epoch_seconds: None,
            usage: None,
        };
        let mut state = PersistedState {
            schema_version: 2,
            grants: vec![
                grant("Forgotten1", Some(now - EXPIRED_GRANT_RETENTION_SECONDS)),
                grant(
                    "RecentlyEx",
                    Some(now - EXPIRED_GRANT_RETENTION_SECONDS + 1),
                ),
                grant("Unlimited1", None),
                grant("Active0001", Some(now + 60)),
            ],
            sessions: Vec::new(),
        };
        assert!(forget_stale_grants(&mut state, now));
        let kept: Vec<_> = state.grants.iter().map(|grant| grant.id.as_str()).collect();
        assert_eq!(kept, ["RecentlyEx", "Unlimited1", "Active0001"]);
        assert!(!forget_stale_grants(&mut state, now));
    }

    fn test_session(id: &str, resource_slot: Option<u32>, expires_at: u64) -> SessionRecord {
        SessionRecord {
            id: id.to_owned(),
            grant_id: "Grant00001".to_owned(),
            secret_hash: None,
            control_secret_hash: Some([0; 32]),
            data_plane_secret_hash: None,
            resource_slot,
            client_name: "Slot test".to_owned(),
            issued_at_epoch_seconds: expires_at.saturating_sub(100),
            expires_at_epoch_seconds: expires_at,
        }
    }
}
