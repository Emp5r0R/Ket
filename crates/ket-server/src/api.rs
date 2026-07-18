use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use ket_core::{
    AccessGrantSummary, CreateAccessGrantRequest, CreateAccessGrantResponse, CreateSessionRequest,
    ErrorResponse, HealthState, NodeStatus, SecretString, SessionManifest, SessionStatus,
    SessionTraffic, SessionTransport, TransportCredential,
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
    service::{AccessService, ServiceError, unix_time},
    system,
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
    metrics: Arc<AppMetrics>,
}

impl AppState {
    pub fn new(config: ServerConfig, access: AccessService) -> anyhow::Result<Self> {
        let data_plane = data_plane::from_config(&config)?;
        Ok(Self {
            config: Arc::new(config),
            access: Arc::new(access),
            data_plane,
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

    async fn kick_sessions(&self, session_ids: &[String]) {
        if let Err(error) = self.data_plane.kick(session_ids).await {
            tracing::warn!(%error, sessions = session_ids.len(), "failed to kick data-plane sessions");
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
    if state.data_plane.healthy().await {
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
    let created = match state
        .access
        .create_session(request.access_code.expose_secret(), request.client_name)
        .await
    {
        Ok(created) => {
            state
                .metrics
                .accepted_sessions
                .fetch_add(1, Ordering::Relaxed);
            created
        }
        Err(error) => {
            state
                .metrics
                .rejected_sessions
                .fetch_add(1, Ordering::Relaxed);
            return Err(error.into());
        }
    };
    let response = SessionManifest {
        session_token: created.token,
        session_expires_at_epoch_seconds: created.expires_at_epoch_seconds,
        node: build_node_status(&state).await,
        transports: session_transports(&state, created.data_plane_token.expose_secret()),
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
    let session_id = state.access.release_session(token).await?;
    state.kick_sessions(&[session_id]).await;
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

fn session_transports(state: &AppState, data_plane_token: &str) -> Vec<SessionTransport> {
    let credential_transport = state
        .config
        .hysteria
        .as_ref()
        .map(|config| config.transport_id.as_str());
    state
        .config
        .transports
        .iter()
        .cloned()
        .map(|profile| {
            let credential = (credential_transport == Some(profile.id.as_str())).then(|| {
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
                TransportCredential {
                    auth: data_plane_token.into(),
                    secrets,
                }
            });
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
    let traffic = match state.data_plane.traffic(&session.id).await {
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
    let data_plane_healthy = state.data_plane.healthy().await;
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
        (
            self.status,
            Json(ErrorResponse {
                code: self.code.to_owned(),
                message: self.message,
            }),
        )
            .into_response()
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
    use ket_core::{
        CreateAccessGrantResponse, ErrorResponse, Network, NodeLocation, NodeStatus,
        SessionManifest, TransportProfile, TransportProtocol,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::{HysteriaConfig, HysteriaObfuscation},
        model::PersistedState,
        repository::{RepositoryError, StateRepository},
    };

    const ADMIN_TOKEN: &str = "test-admin-token-with-at-least-32-characters";

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
