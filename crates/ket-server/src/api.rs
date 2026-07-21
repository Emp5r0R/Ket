use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use ket_core::{
    AccessGrantSummary, CreateAccessGrantBatchRequest, CreateAccessGrantRequest,
    CreateAccessGrantResponse, CreateSessionRequest, ErrorResponse, HealthState, NodeStatus,
    SecretString, SessionManifest, SessionStatus, SessionTraffic, SessionTransport,
    TransportCredential,
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tower_http::{
    catch_panic::CatchPanicLayer, limit::RequestBodyLimitLayer, timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{
    config::{HysteriaObfuscation, ServerConfig},
    data_plane::{self, DataPlaneControl},
    service::{AccessService, CreatedSession, ServiceError, SessionAllocation, unix_time},
    shadowsocks, system, xray,
};

#[derive(Default)]
struct AppMetrics {
    accepted_sessions: AtomicU64,
    rejected_sessions: AtomicU64,
    accepted_data_plane_auth: AtomicU64,
    rejected_data_plane_auth: AtomicU64,
}

#[derive(Clone)]
pub struct AppState {
    config: Arc<ServerConfig>,
    access: Arc<AccessService>,
    data_plane: Arc<dyn DataPlaneControl>,
    data_plane_ready: Arc<AtomicBool>,
    metrics: Arc<AppMetrics>,
}

impl AppState {
    pub fn new(config: ServerConfig, access: AccessService) -> anyhow::Result<Self> {
        let data_plane = data_plane::from_config(&config)?;
        let data_plane_ready = config.xray.is_none() && config.shadowsocks.is_none();
        Ok(Self {
            config: Arc::new(config),
            access: Arc::new(access),
            data_plane,
            data_plane_ready: Arc::new(AtomicBool::new(data_plane_ready)),
            metrics: Arc::new(AppMetrics::default()),
        })
    }

    pub fn start_background_tasks(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                match state.access.expire_sessions().await {
                    Ok(expired) => state.kick_sessions(&expired).await,
                    Err(error) => tracing::error!(%error, "failed to expire session leases"),
                }
            }
        });
    }

    pub async fn reconcile_data_planes(&self) -> anyhow::Result<()> {
        self.data_plane_ready.store(false, Ordering::Release);
        let sessions = self.access.active_session_allocations().await?;
        let mut delay = Duration::from_millis(250);
        for attempt in 1..=20 {
            match self.data_plane.reconcile(&sessions).await {
                Ok(()) => {
                    self.data_plane_ready.store(true, Ordering::Release);
                    tracing::info!(sessions = sessions.len(), "data-plane sessions reconciled");
                    return Ok(());
                }
                Err(error) if attempt < 20 => {
                    tracing::warn!(%error, attempt, "data-plane reconciliation is not ready");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(2));
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("bounded reconciliation loop always returns")
    }

    async fn kick_sessions(&self, sessions: &[SessionAllocation]) {
        if let Err(error) = self.data_plane.kick(sessions).await {
            tracing::warn!(%error, sessions = sessions.len(), "failed to kick data-plane sessions");
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/v1/node/status", get(node_status))
        .route("/internal/v1/hysteria2/auth", post(authenticate_hysteria))
        .route(
            "/v1/admin/access-grants",
            post(create_grant).get(list_grants),
        )
        .route("/v1/admin/access-grants/batch", post(create_grant_batch))
        .route("/v1/admin/access-grants/:id", delete(revoke_grant))
        .route("/v1/sessions", post(create_session))
        .route(
            "/v1/sessions/current",
            get(session_status)
                .put(renew_session)
                .delete(release_session),
        )
        .layer(RequestBodyLimitLayer::new(16 * 1024))
        .layer(TimeoutLayer::new(Duration::from_secs(15)))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if state.data_plane_ready.load(Ordering::Acquire) && state.data_plane.healthy().await {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not_ready", "reason": "data_plane_unhealthy"})),
        )
    }
}

async fn node_status(State(state): State<AppState>) -> Json<NodeStatus> {
    Json(build_node_status(&state).await)
}

async fn create_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAccessGrantRequest>,
) -> Result<(StatusCode, Json<CreateAccessGrantResponse>), ApiError> {
    authorize_admin(&state, &headers)?;
    let response = state.access.create_grant(request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn create_grant_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAccessGrantBatchRequest>,
) -> Result<(StatusCode, Json<Vec<CreateAccessGrantResponse>>), ApiError> {
    authorize_admin(&state, &headers)?;
    let response = state.access.create_grant_batch(request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn list_grants(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccessGrantSummary>>, ApiError> {
    authorize_admin(&state, &headers)?;
    Ok(Json(state.access.list_grants().await))
}

async fn revoke_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    authorize_admin(&state, &headers)?;
    let revoked_sessions = state.access.revoke_grant(&id).await?;
    state.kick_sessions(&revoked_sessions).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionManifest>), ApiError> {
    if !state.data_plane_ready.load(Ordering::Acquire) {
        return Err(ApiError::data_plane_not_ready());
    }
    let created = match state
        .access
        .create_session(request.access_code.expose_secret(), request.client_name)
        .await
    {
        Ok(created) => created,
        Err(error) => {
            state
                .metrics
                .rejected_sessions
                .fetch_add(1, Ordering::Relaxed);
            return Err(error.into());
        }
    };
    let allocation = SessionAllocation {
        id: created.id.clone(),
        resource_slot: created.resource_slot,
    };
    state.kick_sessions(&created.expired_allocations).await;
    if let Err(error) = state.data_plane.provision(&allocation).await {
        state
            .metrics
            .rejected_sessions
            .fetch_add(1, Ordering::Relaxed);
        match state
            .access
            .release_session(created.token.expose_secret())
            .await
        {
            Ok(session) => state.kick_sessions(&[session]).await,
            Err(rollback_error) => {
                tracing::error!(%rollback_error, session_id = %created.id, "failed to roll back unprovisioned session")
            }
        }
        return Err(ApiError::data_plane_unavailable(error));
    }
    state
        .metrics
        .accepted_sessions
        .fetch_add(1, Ordering::Relaxed);
    let transports = session_transports(&state, &created);
    let response = SessionManifest {
        session_token: created.token,
        session_expires_at_epoch_seconds: created.expires_at_epoch_seconds,
        node: build_node_status(&state).await,
        transports,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

async fn session_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionStatus>, ApiError> {
    let token = bearer_token(&headers)?;
    let session = state.access.session(token).await?;
    Ok(Json(build_session_status(&state, session).await))
}

async fn renew_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionStatus>, ApiError> {
    let token = bearer_token(&headers)?;
    let session = state.access.renew_session(token).await?;
    Ok(Json(build_session_status(&state, session).await))
}

async fn release_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let token = bearer_token(&headers)?;
    let session = state.access.release_session(token).await?;
    state.kick_sessions(&[session]).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct HysteriaAuthRequest {
    #[serde(rename = "addr")]
    _remote_address: String,
    auth: SecretString,
    #[serde(rename = "tx")]
    _tx_rate: u64,
}

#[derive(Deserialize, Serialize)]
struct HysteriaAuthResponse {
    ok: bool,
    id: String,
}

async fn authenticate_hysteria(
    State(state): State<AppState>,
    Json(request): Json<HysteriaAuthRequest>,
) -> Json<HysteriaAuthResponse> {
    let session = if state.config.hysteria.is_some() {
        state
            .access
            .authenticate_data_plane(request.auth.expose_secret())
            .await
            .ok()
    } else {
        None
    };
    match session {
        Some(session) => {
            state
                .metrics
                .accepted_data_plane_auth
                .fetch_add(1, Ordering::Relaxed);
            Json(HysteriaAuthResponse {
                ok: true,
                id: session.id,
            })
        }
        None => {
            state
                .metrics
                .rejected_data_plane_auth
                .fetch_add(1, Ordering::Relaxed);
            Json(HysteriaAuthResponse {
                ok: false,
                id: String::new(),
            })
        }
    }
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let active_sessions = state.access.active_session_count().await;
    let session_capacity = state.config.max_sessions;
    let crypto_operations = state.access.crypto_operations_in_flight();
    let crypto_capacity = state.access.crypto_operation_capacity();
    let crypto_overloads = state.access.crypto_overload_count();
    let accepted = state.metrics.accepted_sessions.load(Ordering::Relaxed);
    let rejected = state.metrics.rejected_sessions.load(Ordering::Relaxed);
    let accepted_data_plane = state
        .metrics
        .accepted_data_plane_auth
        .load(Ordering::Relaxed);
    let rejected_data_plane = state
        .metrics
        .rejected_data_plane_auth
        .load(Ordering::Relaxed);
    let body = format!(
        "# HELP ket_active_sessions Current unexpired control-plane sessions.\n\
         # TYPE ket_active_sessions gauge\n\
         ket_active_sessions {active_sessions}\n\
         # HELP ket_session_capacity Configured maximum concurrent sessions.\n\
         # TYPE ket_session_capacity gauge\n\
         ket_session_capacity {session_capacity}\n\
         # HELP ket_crypto_operations Current Argon2 operations running or waiting.\n\
         # TYPE ket_crypto_operations gauge\n\
         ket_crypto_operations {crypto_operations}\n\
         # HELP ket_crypto_operation_capacity Maximum Argon2 operations allowed to run or wait.\n\
         # TYPE ket_crypto_operation_capacity gauge\n\
         ket_crypto_operation_capacity {crypto_capacity}\n\
         # HELP ket_crypto_overload_total Secret-processing operations rejected before queuing.\n\
         # TYPE ket_crypto_overload_total counter\n\
         ket_crypto_overload_total {crypto_overloads}\n\
         # HELP ket_session_exchange_total Session exchange attempts by result.\n\
         # TYPE ket_session_exchange_total counter\n\
         ket_session_exchange_total{{result=\"accepted\"}} {accepted}\n\
         ket_session_exchange_total{{result=\"rejected\"}} {rejected}\n\
         # HELP ket_data_plane_auth_total Hysteria2 authentication attempts by result.\n\
         # TYPE ket_data_plane_auth_total counter\n\
         ket_data_plane_auth_total{{result=\"accepted\"}} {accepted_data_plane}\n\
         ket_data_plane_auth_total{{result=\"rejected\"}} {rejected_data_plane}\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
}

fn session_transports(state: &AppState, created: &CreatedSession) -> Vec<SessionTransport> {
    state
        .config
        .transports
        .iter()
        .cloned()
        .map(|mut profile| {
            let credential = if state
                .config
                .hysteria
                .as_ref()
                .is_some_and(|config| config.transport_id == profile.id)
            {
                let mut secrets = BTreeMap::new();
                if let Some(hysteria) = &state.config.hysteria {
                    match &hysteria.obfuscation {
                        HysteriaObfuscation::Disabled => {}
                        HysteriaObfuscation::Salamander { password }
                        | HysteriaObfuscation::Gecko { password } => {
                            secrets.insert("obfs_password".to_owned(), password.as_str().into());
                        }
                    }
                }
                Some(TransportCredential {
                    auth: created.data_plane_token.clone(),
                    secrets,
                })
            } else if let Some(shadowsocks) = state
                .config
                .shadowsocks
                .as_ref()
                .filter(|config| config.transport_id == profile.id)
            {
                profile.port = shadowsocks
                    .session_port(created.resource_slot)
                    .expect("validated Shadowsocks pool covers every session resource slot");
                Some(shadowsocks::session_credential(shadowsocks, &created.id))
            } else if let Some(wireguard) = state
                .config
                .wireguard
                .as_ref()
                .filter(|config| config.transport_id == profile.id)
            {
                profile.options.insert(
                    "client_address".to_owned(),
                    wireguard
                        .client_address(created.resource_slot)
                        .expect("validated WireGuard capacity covers every session resource slot"),
                );
                Some(crate::wireguard::session_credential(wireguard, &created.id))
            } else {
                state
                    .config
                    .xray
                    .as_ref()
                    .and_then(|config| xray::session_credential(config, &profile.id, &created.id))
            };
            SessionTransport {
                credential,
                profile,
            }
        })
        .collect()
}

async fn build_session_status(
    state: &AppState,
    session: crate::service::SessionView,
) -> SessionStatus {
    let allocation = SessionAllocation {
        id: session.id.clone(),
        resource_slot: session.resource_slot,
    };
    let traffic = match state.data_plane.traffic(&allocation).await {
        Ok(traffic) => traffic,
        Err(error) => {
            tracing::warn!(%error, session_id = %session.id, "data-plane traffic is unavailable");
            SessionTraffic {
                available: false,
                bytes_sent: 0,
                bytes_received: 0,
                online_connections: 0,
                observed_at_epoch_seconds: unix_time(),
            }
        }
    };
    SessionStatus {
        session_id: session.id,
        client_name: session.client_name,
        expires_at_epoch_seconds: session.expires_at_epoch_seconds,
        node: build_node_status(state).await,
        traffic,
    }
}

async fn build_node_status(state: &AppState) -> NodeStatus {
    let active_sessions = state.access.active_session_count().await;
    let capacity_percent = active_sessions as f32 / state.config.max_sessions as f32 * 100.0;
    let system = system::snapshot();
    let data_plane_healthy =
        state.data_plane_ready.load(Ordering::Acquire) && state.data_plane.healthy().await;
    let memory_percent = match (system.memory_used_bytes, system.memory_total_bytes) {
        (Some(used), Some(total)) if total > 0 => used as f64 / total as f64 * 100.0,
        _ => 0.0,
    };
    let health = if capacity_percent >= 100.0 {
        HealthState::Saturated
    } else if !data_plane_healthy
        || capacity_percent >= 85.0
        || system.cpu_load_percent.is_some_and(|load| load >= 90.0)
        || memory_percent >= 95.0
    {
        HealthState::Degraded
    } else {
        HealthState::Healthy
    };
    NodeStatus {
        node_id: state.config.node_id.clone(),
        display_name: state.config.node_name.clone(),
        public_url: state.config.public_url.clone(),
        location: state.config.location.clone(),
        health,
        active_sessions,
        session_capacity: state.config.max_sessions,
        capacity_percent,
        cpu_load_percent: system.cpu_load_percent,
        memory_used_bytes: system.memory_used_bytes,
        memory_total_bytes: system.memory_total_bytes,
        uptime_seconds: system.uptime_seconds,
        observed_at_epoch_seconds: unix_time(),
    }
}

fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = bearer_token(headers)?;
    let expected = state.config.admin_token.as_bytes();
    let supplied = supplied.as_bytes();
    if expected.len() == supplied.len() && bool::from(expected.ct_eq(supplied)) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(ApiError::unauthorized)
}

struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "credentials are invalid".to_owned(),
        }
    }

    fn data_plane_unavailable(error: data_plane::DataPlaneError) -> Self {
        tracing::error!(%error, "session data-plane provisioning failed");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "data_plane_unavailable",
            message: "the node data plane is temporarily unavailable".to_owned(),
        }
    }

    fn data_plane_not_ready() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "data_plane_unavailable",
            message: "the node data plane is temporarily unavailable".to_owned(),
        }
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::InvalidInput(message) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "invalid_request",
                message,
            },
            ServiceError::Unauthorized => Self::unauthorized(),
            ServiceError::GrantExpired => Self {
                status: StatusCode::FORBIDDEN,
                code: "grant_expired",
                message: error.to_string(),
            },
            ServiceError::GrantCapacity => Self {
                status: StatusCode::CONFLICT,
                code: "grant_capacity_reached",
                message: error.to_string(),
            },
            ServiceError::NodeCapacity => Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "node_capacity_reached",
                message: error.to_string(),
            },
            ServiceError::Busy => Self {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "server_busy",
                message: "the server is temporarily busy; retry shortly".to_owned(),
            },
            ServiceError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
                message: error.to_string(),
            },
            internal => {
                tracing::error!(error = %internal, "request failed");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "internal_error",
                    message: "the server could not complete the request".to_owned(),
                }
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let retry_after = self.status == StatusCode::TOO_MANY_REQUESTS;
        let mut response = (
            self.status,
            Json(ErrorResponse {
                code: self.code.to_owned(),
                message: self.message,
            }),
        )
            .into_response();
        if retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ket_core::{
        CreateAccessGrantResponse, ErrorResponse, Network, NodeLocation, NodeStatus,
        SessionManifest, TransportProfile, TransportProtocol,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::{
            HysteriaConfig, HysteriaObfuscation, SHADOWSOCKS_2022_METHOD, ShadowsocksConfig,
            WireGuardConfig, XrayConfig, XrayRealityConfig, XrayXhttpConfig,
        },
        data_plane::DataPlaneError,
        model::PersistedState,
        repository::{RepositoryError, StateRepository},
    };

    const ADMIN_TOKEN: &str = "test-admin-token-with-at-least-32-characters";

    #[tokio::test]
    async fn crypto_overload_returns_a_retryable_http_contract() {
        let response = ApiError::from(ServiceError::Busy).into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::RETRY_AFTER),
            Some(&HeaderValue::from_static("1"))
        );
        let body = to_bytes(response.into_body(), 1024)
            .await
            .expect("error response should be readable");
        let error: ErrorResponse =
            serde_json::from_slice(&body).expect("error response should remain valid JSON");
        assert_eq!(error.code, "server_busy");
    }

    #[derive(Default)]
    struct MemoryRepository {
        state: Mutex<PersistedState>,
    }

    impl MemoryRepository {
        fn encoded_state(&self) -> String {
            serde_json::to_string(&*self.state.lock().expect("state lock"))
                .expect("serialize state")
        }
    }

    #[async_trait]
    impl StateRepository for MemoryRepository {
        async fn load(&self) -> Result<PersistedState, RepositoryError> {
            Ok(self.state.lock().expect("state lock").clone())
        }

        async fn store(&self, state: &PersistedState) -> Result<(), RepositoryError> {
            *self.state.lock().expect("state lock") = state.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingDataPlane {
        fail_provision: bool,
        provision_attempts: Mutex<Vec<String>>,
        kicked_sessions: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DataPlaneControl for RecordingDataPlane {
        async fn healthy(&self) -> bool {
            true
        }

        async fn provision(&self, session: &SessionAllocation) -> Result<(), DataPlaneError> {
            self.provision_attempts
                .lock()
                .expect("provision attempts lock")
                .push(session.id.clone());
            if self.fail_provision {
                Err(DataPlaneError::Command("injected failure".to_owned()))
            } else {
                Ok(())
            }
        }

        async fn reconcile(&self, _sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
            Ok(())
        }

        async fn traffic(
            &self,
            _session: &SessionAllocation,
        ) -> Result<SessionTraffic, DataPlaneError> {
            Ok(SessionTraffic {
                available: true,
                bytes_sent: 0,
                bytes_received: 0,
                online_connections: 0,
                observed_at_epoch_seconds: unix_time(),
            })
        }

        async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
            self.kicked_sessions
                .lock()
                .expect("kicked sessions lock")
                .extend(sessions.iter().map(|session| session.id.clone()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn complete_grant_and_session_lifecycle_enforces_limits() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository.clone(), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let app = build_router(AppState::new(test_config(), service).expect("app state"));

        let unauthorized = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                None,
                serde_json::json!({
                    "label": "Personal devices",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "Personal devices",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(grant_response.status(), StatusCode::CREATED);
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;
        assert_eq!(grant.access_code.len(), ket_core::ACCESS_CODE_LENGTH);
        assert!(
            !repository
                .encoded_state()
                .contains(grant.access_code.expose_secret())
        );

        let batch_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants/batch",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label_prefix": "Fleet",
                    "count": 2,
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(batch_response.status(), StatusCode::CREATED);
        let batch: Vec<CreateAccessGrantResponse> = response_json(batch_response).await;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].access_code.len(), ket_core::ACCESS_CODE_LENGTH);
        assert_ne!(batch[0].access_code, batch[1].access_code);

        let session_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "Linux workstation"
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(session_response.status(), StatusCode::CREATED);
        let session: SessionManifest = response_json(session_response).await;
        assert!(
            !repository
                .encoded_state()
                .contains(session.session_token.expose_secret())
        );

        let at_capacity = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "Android phone"
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(at_capacity.status(), StatusCode::CONFLICT);
        let error: ErrorResponse = response_json(at_capacity).await;
        assert_eq!(error.code, "grant_capacity_reached");

        let status_response = app
            .clone()
            .oneshot(empty_request(
                "GET",
                "/v1/sessions/current",
                Some(session.session_token.expose_secret()),
            ))
            .await
            .expect("request should complete");
        assert_eq!(status_response.status(), StatusCode::OK);
        let status: SessionStatus = response_json(status_response).await;
        assert_eq!(status.client_name, "Linux workstation");
        assert_eq!(status.node.active_sessions, 1);
        assert_eq!(status.node.capacity_percent, 25.0);

        let release = app
            .clone()
            .oneshot(empty_request(
                "DELETE",
                "/v1/sessions/current",
                Some(session.session_token.expose_secret()),
            ))
            .await
            .expect("request should complete");
        assert_eq!(release.status(), StatusCode::NO_CONTENT);

        let released_token = app
            .clone()
            .oneshot(empty_request(
                "GET",
                "/v1/sessions/current",
                Some(session.session_token.expose_secret()),
            ))
            .await
            .expect("request should complete");
        assert_eq!(released_token.status(), StatusCode::UNAUTHORIZED);

        let revoke = app
            .oneshot(empty_request(
                "DELETE",
                &format!("/v1/admin/access-grants/{}", grant.id),
                Some(ADMIN_TOKEN),
            ))
            .await
            .expect("request should complete");
        assert_eq!(revoke.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn node_status_exposes_map_and_capacity_data() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let app = build_router(AppState::new(test_config(), service).expect("app state"));

        let response = app
            .oneshot(empty_request("GET", "/v1/node/status", None))
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        let status: NodeStatus = response_json(response).await;
        assert_eq!(status.location.country_code, "NL");
        assert_eq!(status.location.latitude, 52.3676);
        assert_eq!(status.session_capacity, 4);
    }

    #[tokio::test]
    async fn prometheus_metrics_expose_capacity_and_crypto_pressure() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let app = build_router(AppState::new(test_config(), service).expect("app state"));

        let response = app
            .oneshot(empty_request("GET", "/metrics", None))
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/plain; version=0.0.4"))
        );
        let body = to_bytes(response.into_body(), 16 * 1024)
            .await
            .expect("metrics response should be readable");
        let body = std::str::from_utf8(&body).expect("metrics should be UTF-8");

        for sample in [
            "ket_active_sessions 0",
            "ket_session_capacity 4",
            "ket_crypto_operations 0",
            "ket_crypto_operation_capacity 32",
            "ket_crypto_overload_total 0",
            "ket_session_exchange_total{result=\"accepted\"} 0",
            "ket_data_plane_auth_total{result=\"rejected\"} 0",
        ] {
            assert!(body.lines().any(|line| line == sample), "missing {sample}");
        }
    }

    #[tokio::test]
    async fn hysteria_uses_a_scoped_credential_and_rejects_released_sessions() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository.clone(), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let state = AppState::new(hysteria_test_config(), service).expect("app state");
        let app = build_router(state.clone());

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "Hysteria test",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let session_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "Hysteria client"
                }),
            ))
            .await
            .expect("request should complete");
        let session: SessionManifest = response_json(session_response).await;
        let data_plane_token = session.transports[0]
            .credential
            .as_ref()
            .expect("Hysteria must receive a credential")
            .auth
            .clone();
        assert_ne!(data_plane_token, session.session_token);
        assert_eq!(
            &data_plane_token.expose_secret()[..12],
            &session.session_token.expose_secret()[..12]
        );
        assert!(
            !repository
                .encoded_state()
                .contains(data_plane_token.expose_secret())
        );

        let accepted = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/internal/v1/hysteria2/auth",
                None,
                serde_json::json!({
                    "addr": "192.0.2.10:12345",
                    "auth": data_plane_token,
                    "tx": 0
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(accepted.status(), StatusCode::OK);
        let accepted: HysteriaAuthResponse = response_json(accepted).await;
        assert!(accepted.ok);
        assert_eq!(accepted.id, &session.session_token.expose_secret()[..12]);

        let control_token_is_rejected = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/internal/v1/hysteria2/auth",
                None,
                serde_json::json!({
                    "addr": "192.0.2.10:12345",
                    "auth": session.session_token,
                    "tx": 0
                }),
            ))
            .await
            .expect("request should complete");
        let rejected: HysteriaAuthResponse = response_json(control_token_is_rejected).await;
        assert!(!rejected.ok);

        state
            .access
            .release_session(session.session_token.expose_secret())
            .await
            .expect("release should succeed");
        let after_release = app
            .oneshot(json_request(
                "POST",
                "/internal/v1/hysteria2/auth",
                None,
                serde_json::json!({
                    "addr": "192.0.2.10:12345",
                    "auth": data_plane_token,
                    "tx": 0
                }),
            ))
            .await
            .expect("request should complete");
        let rejected: HysteriaAuthResponse = response_json(after_release).await;
        assert!(!rejected.ok);
    }

    #[tokio::test]
    async fn xray_readiness_blocks_session_exchange_until_reconciliation() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let state = AppState::new(xray_test_config(), service).expect("app state");
        let app = build_router(state.clone());

        let readiness = app
            .clone()
            .oneshot(empty_request("GET", "/readyz", None))
            .await
            .expect("request should complete");
        assert_eq!(readiness.status(), StatusCode::SERVICE_UNAVAILABLE);

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "Startup gate test",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let response = app
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "Early client"
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error: ErrorResponse = response_json(response).await;
        assert_eq!(error.code, "data_plane_unavailable");
        assert_eq!(state.access.active_session_count().await, 0);
    }

    #[tokio::test]
    async fn xray_session_provisions_and_revokes_all_transport_credentials() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let data_plane = Arc::new(RecordingDataPlane::default());
        let state = AppState {
            config: Arc::new(xray_test_config()),
            access: Arc::new(service),
            data_plane: data_plane.clone(),
            data_plane_ready: Arc::new(AtomicBool::new(true)),
            metrics: Arc::new(AppMetrics::default()),
        };
        let app = build_router(state);

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "REALITY test",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let session_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "REALITY client"
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(session_response.status(), StatusCode::CREATED);
        let session: SessionManifest = response_json(session_response).await;
        let session_id = session.session_token.expose_secret()[..12].to_owned();
        let transport = session
            .transports
            .iter()
            .find(|transport| transport.profile.protocol == TransportProtocol::VlessXtlsReality)
            .expect("REALITY transport");
        let credential = transport.credential.as_ref().expect("REALITY credential");
        let uuid = credential.auth.expose_secret();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.as_bytes()[14], b'4');
        assert_eq!(
            uuid.split('-').map(str::len).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert_eq!(
            credential
                .secrets
                .get("reality_password")
                .expect("REALITY public key")
                .expose_secret(),
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
        );
        assert_eq!(
            credential
                .secrets
                .get("reality_short_id")
                .expect("REALITY short ID")
                .expose_secret(),
            "0123456789abcdef"
        );
        let stealth = session
            .transports
            .iter()
            .find(|transport| transport.profile.protocol == TransportProtocol::Stealth)
            .expect("Stealth transport");
        let stealth_credential = stealth.credential.as_ref().expect("Stealth credential");
        assert_eq!(stealth_credential.auth.expose_secret(), uuid);
        assert!(stealth_credential.secrets.is_empty());
        assert_eq!(
            data_plane
                .provision_attempts
                .lock()
                .expect("provision attempts lock")
                .as_slice(),
            [session_id.as_str()]
        );

        let release = app
            .oneshot(empty_request(
                "DELETE",
                "/v1/sessions/current",
                Some(session.session_token.expose_secret()),
            ))
            .await
            .expect("request should complete");
        assert_eq!(release.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            data_plane
                .kicked_sessions
                .lock()
                .expect("kicked sessions lock")
                .as_slice(),
            [session_id.as_str()]
        );
    }

    #[tokio::test]
    async fn shadowsocks_sessions_receive_distinct_keys_and_ports() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository.clone(), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let data_plane = Arc::new(RecordingDataPlane::default());
        let state = AppState {
            config: Arc::new(shadowsocks_test_config()),
            access: Arc::new(service),
            data_plane,
            data_plane_ready: Arc::new(AtomicBool::new(true)),
            metrics: Arc::new(AppMetrics::default()),
        };
        let app = build_router(state);

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "Shadowsocks test",
                    "max_connections": 2,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let mut sessions = Vec::new();
        for client_name in ["First Shadowsocks client", "Second Shadowsocks client"] {
            let response = app
                .clone()
                .oneshot(json_request(
                    "POST",
                    "/v1/sessions",
                    None,
                    serde_json::json!({
                        "access_code": grant.access_code,
                        "client_name": client_name
                    }),
                ))
                .await
                .expect("request should complete");
            assert_eq!(response.status(), StatusCode::CREATED);
            sessions.push(response_json::<SessionManifest>(response).await);
        }

        let transports = sessions
            .iter()
            .map(|session| {
                session
                    .transports
                    .iter()
                    .find(|transport| {
                        transport.profile.protocol == TransportProtocol::Shadowsocks2022
                    })
                    .expect("Shadowsocks transport")
            })
            .collect::<Vec<_>>();
        assert_eq!(transports[0].profile.port, 20_000);
        assert_eq!(transports[1].profile.port, 20_001);
        let first_key = transports[0]
            .credential
            .as_ref()
            .expect("Shadowsocks credential")
            .auth
            .expose_secret();
        let second_key = transports[1]
            .credential
            .as_ref()
            .expect("Shadowsocks credential")
            .auth
            .expose_secret();
        assert_ne!(first_key, second_key);
        assert_eq!(STANDARD.decode(first_key).expect("base64 key").len(), 32);
        assert_eq!(STANDARD.decode(second_key).expect("base64 key").len(), 32);
        assert!(
            transports.iter().all(|transport| transport
                .credential
                .as_ref()
                .unwrap()
                .secrets
                .is_empty())
        );
        assert!(!repository.encoded_state().contains(first_key));
        assert!(!repository.encoded_state().contains(second_key));
    }

    #[tokio::test]
    async fn wireguard_sessions_receive_distinct_keys_and_addresses() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository.clone(), Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let state = AppState {
            config: Arc::new(wireguard_test_config()),
            access: Arc::new(service),
            data_plane: Arc::new(RecordingDataPlane::default()),
            data_plane_ready: Arc::new(AtomicBool::new(true)),
            metrics: Arc::new(AppMetrics::default()),
        };
        let app = build_router(state);

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "WireGuard test",
                    "max_connections": 2,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let mut transports = Vec::new();
        for client_name in ["First WireGuard client", "Second WireGuard client"] {
            let response = app
                .clone()
                .oneshot(json_request(
                    "POST",
                    "/v1/sessions",
                    None,
                    serde_json::json!({
                        "access_code": grant.access_code,
                        "client_name": client_name
                    }),
                ))
                .await
                .expect("request should complete");
            assert_eq!(response.status(), StatusCode::CREATED);
            let session: SessionManifest = response_json(response).await;
            transports.push(
                session
                    .transports
                    .into_iter()
                    .find(|transport| transport.profile.protocol == TransportProtocol::WireGuard)
                    .expect("WireGuard transport"),
            );
        }

        let first_credential = transports[0].credential.as_ref().expect("first credential");
        let second_credential = transports[1]
            .credential
            .as_ref()
            .expect("second credential");
        assert_ne!(first_credential.auth, second_credential.auth);
        assert_ne!(
            first_credential.secrets["preshared_key"],
            second_credential.secrets["preshared_key"]
        );
        assert_eq!(
            first_credential.secrets["server_public_key"],
            second_credential.secrets["server_public_key"]
        );
        assert_eq!(transports[0].profile.options["client_address"], "10.66.0.2");
        assert_eq!(transports[1].profile.options["client_address"], "10.66.0.3");
        for credential in [first_credential, second_credential] {
            assert_eq!(credential.auth.expose_secret().len(), 44);
            assert_eq!(
                credential.secrets["preshared_key"].expose_secret().len(),
                44
            );
            assert!(
                !repository
                    .encoded_state()
                    .contains(credential.auth.expose_secret())
            );
            assert!(
                !repository
                    .encoded_state()
                    .contains(credential.secrets["preshared_key"].expose_secret())
            );
        }
    }

    #[tokio::test]
    async fn failed_data_plane_provision_rolls_back_the_session() {
        let repository = Arc::new(MemoryRepository::default());
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let data_plane = Arc::new(RecordingDataPlane {
            fail_provision: true,
            ..RecordingDataPlane::default()
        });
        let state = AppState {
            config: Arc::new(xray_test_config()),
            access: Arc::new(service),
            data_plane: data_plane.clone(),
            data_plane_ready: Arc::new(AtomicBool::new(true)),
            metrics: Arc::new(AppMetrics::default()),
        };
        let app = build_router(state.clone());

        let grant_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                serde_json::json!({
                    "label": "Provision failure test",
                    "max_connections": 1,
                    "expires_at_epoch_seconds": null
                }),
            ))
            .await
            .expect("request should complete");
        let grant: CreateAccessGrantResponse = response_json(grant_response).await;

        let response = app
            .oneshot(json_request(
                "POST",
                "/v1/sessions",
                None,
                serde_json::json!({
                    "access_code": grant.access_code,
                    "client_name": "Unavailable client"
                }),
            ))
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error: ErrorResponse = response_json(response).await;
        assert_eq!(error.code, "data_plane_unavailable");
        assert_eq!(state.access.active_session_count().await, 0);
        assert_eq!(
            data_plane
                .provision_attempts
                .lock()
                .expect("provision attempts lock")
                .len(),
            1
        );
        assert_eq!(
            data_plane
                .kicked_sessions
                .lock()
                .expect("kicked sessions lock")
                .len(),
            1
        );
        assert_eq!(state.metrics.accepted_sessions.load(Ordering::Relaxed), 0);
        assert_eq!(state.metrics.rejected_sessions.load(Ordering::Relaxed), 1);
    }

    fn test_config() -> ServerConfig {
        ServerConfig {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8787),
            state_path: PathBuf::from("unused-in-tests.json"),
            admin_token: ADMIN_TOKEN.to_owned(),
            public_url: "https://nl.example.test".to_owned(),
            node_id: "nl-ams-1".to_owned(),
            node_name: "Amsterdam".to_owned(),
            location: NodeLocation {
                country_code: "NL".to_owned(),
                country_name: "Netherlands".to_owned(),
                city: Some("Amsterdam".to_owned()),
                latitude: 52.3676,
                longitude: 4.9041,
            },
            max_sessions: 4,
            session_ttl: Duration::from_secs(300),
            transports: Vec::new(),
            hysteria: None,
            shadowsocks: None,
            wireguard: None,
            xray: None,
        }
    }

    fn hysteria_test_config() -> ServerConfig {
        let hysteria = HysteriaConfig {
            transport_id: "hy2-primary".to_owned(),
            runtime_config_path: PathBuf::from("unused-hysteria-test.json"),
            listen: ":8443".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            public_port: 443,
            sni: "vpn.example.test".to_owned(),
            tls_cert_path: "/tls/fullchain.pem".to_owned(),
            tls_key_path: "/tls/privkey.pem".to_owned(),
            auth_url: "http://control-plane:8787/internal/v1/hysteria2/auth".to_owned(),
            stats_url: "http://127.0.0.1:9".to_owned(),
            stats_secret: "stats-secret-at-least-thirty-two-characters".to_owned(),
            masquerade_url: "https://example.com/".to_owned(),
            obfuscation: HysteriaObfuscation::Disabled,
        };
        let mut config = test_config();
        config.transports.push(TransportProfile {
            id: hysteria.transport_id.clone(),
            display_name: "Hysteria 2".to_owned(),
            protocol: TransportProtocol::Hysteria2,
            endpoint: hysteria.public_host.clone(),
            port: hysteria.public_port,
            network: Network::Udp,
            priority: 10,
            tls_server_name: Some(hysteria.sni.clone()),
            options: Default::default(),
        });
        config.hysteria = Some(hysteria);
        config
    }

    fn shadowsocks_test_config() -> ServerConfig {
        let shadowsocks = ShadowsocksConfig {
            transport_id: "shadowsocks-2022-primary".to_owned(),
            manager_address: "127.0.0.1:6100".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            port_start: 20_000,
            port_end: 20_003,
            credential_key: "test-shadowsocks-key-with-at-least-32-characters".to_owned(),
        };
        let mut config = test_config();
        config.transports.push(TransportProfile {
            id: shadowsocks.transport_id.clone(),
            display_name: "Shadowsocks 2022".to_owned(),
            protocol: TransportProtocol::Shadowsocks2022,
            endpoint: shadowsocks.public_host.clone(),
            port: shadowsocks.port_start,
            network: Network::TcpAndUdp,
            priority: 15,
            tls_server_name: None,
            options: BTreeMap::from([
                ("method".to_owned(), SHADOWSOCKS_2022_METHOD.to_owned()),
                ("mode".to_owned(), "tcp_and_udp".to_owned()),
                ("port_allocation".to_owned(), "lease_slot".to_owned()),
            ]),
        });
        config.shadowsocks = Some(shadowsocks);
        config
    }

    fn wireguard_test_config() -> ServerConfig {
        let wireguard = WireGuardConfig {
            transport_id: "wireguard-tls-primary".to_owned(),
            manager_url: "http://127.0.0.1:8788".to_owned(),
            manager_token: "test-manager-token-with-at-least-32-characters".to_owned(),
            credential_key: "test-wireguard-key-with-at-least-32-characters".to_owned(),
            server_public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            public_host: "wireguard.example.test".to_owned(),
            public_port: 443,
            sni: "wireguard.example.test".to_owned(),
            path_prefix: "ket-wireguard-test".to_owned(),
        };
        let mut config = test_config();
        config.transports.push(TransportProfile {
            id: wireguard.transport_id.clone(),
            display_name: "WireGuard TLS".to_owned(),
            protocol: TransportProtocol::WireGuard,
            endpoint: wireguard.public_host.clone(),
            port: wireguard.public_port,
            network: Network::Tcp,
            priority: 2,
            tls_server_name: Some(wireguard.sni.clone()),
            options: BTreeMap::from([
                ("address_allocation".to_owned(), "lease_slot".to_owned()),
                ("allowed_ips".to_owned(), "0.0.0.0/0".to_owned()),
                ("keepalive_seconds".to_owned(), "25".to_owned()),
                ("mtu".to_owned(), "1280".to_owned()),
                ("path_prefix".to_owned(), wireguard.path_prefix.clone()),
                (
                    "remote_address".to_owned(),
                    "wireguard-agent:51820".to_owned(),
                ),
                ("transport".to_owned(), "websocket_tls".to_owned()),
            ]),
        });
        config.wireguard = Some(wireguard);
        config
    }

    fn xray_test_config() -> ServerConfig {
        let reality = XrayRealityConfig {
            transport_id: "vless-reality-primary".to_owned(),
            inbound_tag: "vless-reality".to_owned(),
            listen_host: "0.0.0.0".to_owned(),
            listen_port: 8444,
            public_host: "vpn.example.test".to_owned(),
            public_port: 443,
            sni: "www.example.com".to_owned(),
            server_names: vec!["www.example.com".to_owned()],
            reality_target: "www.example.com:443".to_owned(),
            private_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            public_key: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
            short_id: "0123456789abcdef".to_owned(),
            fingerprint: "chrome".to_owned(),
        };
        let xray = XrayConfig {
            runtime_config_path: PathBuf::from("unused-xray-test.json"),
            binary_path: PathBuf::from("unused-xray-test-binary"),
            api_server: "127.0.0.1:10085".to_owned(),
            api_listen: "127.0.0.1".to_owned(),
            api_port: 10085,
            credential_key: "test-credential-key-with-at-least-32-characters".to_owned(),
            reality: Some(reality.clone()),
            xhttp: Some(XrayXhttpConfig {
                transport_id: "https-stealth-primary".to_owned(),
                inbound_tag: "vless-xhttp".to_owned(),
                listen_host: "0.0.0.0".to_owned(),
                listen_port: 8445,
                public_host: "stealth.example.test".to_owned(),
                public_port: 443,
                sni: "stealth.example.test".to_owned(),
                path: "/a1b2c3d4e5f6g7h8".to_owned(),
                fingerprint: "chrome".to_owned(),
            }),
        };
        let options = BTreeMap::from([
            ("encryption".to_owned(), "none".to_owned()),
            ("fingerprint".to_owned(), reality.fingerprint.clone()),
            ("flow".to_owned(), "xtls-rprx-vision".to_owned()),
            ("transport".to_owned(), "raw".to_owned()),
        ]);
        let mut config = test_config();
        config.transports.push(TransportProfile {
            id: reality.transport_id.clone(),
            display_name: "VLESS + REALITY".to_owned(),
            protocol: TransportProtocol::VlessXtlsReality,
            endpoint: reality.public_host.clone(),
            port: reality.public_port,
            network: Network::Tcp,
            priority: 5,
            tls_server_name: Some(reality.sni.clone()),
            options,
        });
        config.transports.push(TransportProfile {
            id: "https-stealth-primary".to_owned(),
            display_name: "HTTPS Stealth".to_owned(),
            protocol: TransportProtocol::Stealth,
            endpoint: "stealth.example.test".to_owned(),
            port: 443,
            network: Network::Tcp,
            priority: 1,
            tls_server_name: Some("stealth.example.test".to_owned()),
            options: BTreeMap::from([
                ("encryption".to_owned(), "none".to_owned()),
                ("fingerprint".to_owned(), "chrome".to_owned()),
                ("mode".to_owned(), "packet-up".to_owned()),
                ("path".to_owned(), "/a1b2c3d4e5f6g7h8".to_owned()),
                ("security".to_owned(), "tls".to_owned()),
                ("transport".to_owned(), "xhttp".to_owned()),
            ]),
        });
        config.xray = Some(xray);
        config
    }

    fn json_request(
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: serde_json::Value,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        request
            .body(Body::from(body.to_string()))
            .expect("valid request")
    }

    fn empty_request(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        request.body(Body::empty()).expect("valid request")
    }

    async fn response_json<T: serde::de::DeserializeOwned>(response: Response) -> T {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("JSON response")
    }
}
