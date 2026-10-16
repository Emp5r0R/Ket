//! Admin routes for access grants (invites): create, list, inspect, change,
//! and revoke. Every route requires the admin token and is served only on
//! the loopback control port, never through the public edge.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use ket_core::{
    AccessGrantDetails, AccessGrantSummary, ConnectedDevice, CreateAccessGrantBatchRequest,
    CreateAccessGrantRequest, CreateAccessGrantResponse, OfferedProtocol, ProtocolAccess,
    UpdateAccessGrantRequest, UpdateAccessGrantResponse,
};

use super::{ApiError, AppState, authorize_admin, unavailable_traffic};
use crate::config::ServerConfig;

pub(super) async fn offered_protocols(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<OfferedProtocol>>, ApiError> {
    authorize_admin(&state, &headers)?;
    Ok(Json(protocols_offered_by(&state.config)))
}

pub(super) async fn create_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAccessGrantRequest>,
) -> Result<(StatusCode, Json<CreateAccessGrantResponse>), ApiError> {
    authorize_admin(&state, &headers)?;
    require_offered(&state.config, &request.protocols)?;
    let mut response = state.access.create_grant(request).await?;
    attach_invite(&state, &mut response);
    Ok((StatusCode::CREATED, Json(response)))
}

pub(super) async fn create_grant_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAccessGrantBatchRequest>,
) -> Result<(StatusCode, Json<Vec<CreateAccessGrantResponse>>), ApiError> {
    authorize_admin(&state, &headers)?;
    require_offered(&state.config, &request.protocols)?;
    let mut response = state.access.create_grant_batch(request).await?;
    for grant in &mut response {
        attach_invite(&state, grant);
    }
    Ok((StatusCode::CREATED, Json(response)))
}

/// Wraps a new grant's access code in a signed invite when the node has a
/// signing key. The grant already exists, so a failure is logged and the
/// access code is still returned rather than lost.
fn attach_invite(state: &AppState, grant: &mut CreateAccessGrantResponse) {
    let Some(issuer) = &state.config.invite_issuer else {
        return;
    };
    match issuer.issue(&grant.access_code, grant.expires_at_epoch_seconds) {
        Ok(invite) => grant.invite = Some(invite),
        Err(error) => tracing::error!(%error, grant_id = %grant.id, "failed to issue an invite"),
    }
}

pub(super) async fn list_grants(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccessGrantSummary>>, ApiError> {
    authorize_admin(&state, &headers)?;
    Ok(Json(state.access.list_grants().await))
}

pub(super) async fn grant_details(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<AccessGrantDetails>, ApiError> {
    authorize_admin(&state, &headers)?;
    let (grant, devices) = state.access.grant_details(&id).await?;
    let mut connected = Vec::with_capacity(devices.len());
    for device in devices {
        let traffic = match state.data_plane.traffic(&device.allocation).await {
            Ok(traffic) => traffic,
            Err(error) => {
                tracing::warn!(%error, session_id = %device.allocation.id, "data-plane traffic is unavailable");
                unavailable_traffic()
            }
        };
        connected.push(ConnectedDevice {
            session_id: device.allocation.id,
            client_name: device.client_name,
            connected_at_epoch_seconds: device.connected_at_epoch_seconds,
            session_expires_at_epoch_seconds: device.session_expires_at_epoch_seconds,
            traffic,
        });
    }
    Ok(Json(AccessGrantDetails {
        grant,
        devices: connected,
    }))
}

/// Applies the change and disconnects the devices it ended; they connect
/// again with the new settings or are refused.
pub(super) async fn update_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateAccessGrantRequest>,
) -> Result<Json<UpdateAccessGrantResponse>, ApiError> {
    authorize_admin(&state, &headers)?;
    if let Some(protocols) = &request.protocols {
        require_offered(&state.config, protocols)?;
    }
    let (grant, ended) = state.access.update_grant(&id, request).await?;
    state.kick_sessions(&ended).await;
    Ok(Json(UpdateAccessGrantResponse {
        grant,
        disconnected_devices: ended.len() as u32,
    }))
}

pub(super) async fn revoke_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    authorize_admin(&state, &headers)?;
    let revoked_sessions = state.access.revoke_grant(&id).await?;
    state.kick_sessions(&revoked_sessions).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The protocols of the transports this server runs, in its preference
/// order, each once.
fn protocols_offered_by(config: &ServerConfig) -> Vec<OfferedProtocol> {
    let mut transports = config.transports.iter().collect::<Vec<_>>();
    transports.sort_by_key(|transport| transport.priority);
    let mut offered: Vec<OfferedProtocol> = Vec::new();
    for transport in transports {
        if offered
            .iter()
            .any(|known| known.protocol == transport.protocol)
        {
            continue;
        }
        offered.push(OfferedProtocol {
            protocol: transport.protocol.clone(),
            name: transport.protocol.short_name().to_owned(),
            display_name: transport.display_name.clone(),
        });
    }
    offered
}

/// A grant may be limited only to protocols this server offers, so a typo
/// cannot create an invite that admits no device.
fn require_offered(config: &ServerConfig, protocols: &ProtocolAccess) -> Result<(), ApiError> {
    let ProtocolAccess::Only(requested) = protocols else {
        return Ok(());
    };
    let offered = protocols_offered_by(config);
    let missing = requested
        .iter()
        .filter(|protocol| !offered.iter().any(|known| &known.protocol == *protocol))
        .map(|protocol| protocol.short_name())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            format!("this server does not offer: {}", missing.join(", ")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };

    use axum::{Router, body::Body, http::Request, response::Response};
    use ket_core::{ErrorResponse, Network, SessionManifest, TransportProfile, TransportProtocol};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        api::{
            AppMetrics, HysteriaAuthResponse, build_router,
            tests::{
                ADMIN_TOKEN, MemoryRepository, RecordingDataPlane, empty_request,
                hysteria_test_config, json_request, response_json, test_config,
            },
        },
        service::{AccessService, unix_time as unix_now},
    };

    struct Harness {
        app: Router,
        state: AppState,
        data_plane: Arc<RecordingDataPlane>,
    }

    /// A server offering Hysteria 2 (priority 10) and HTTPS Stealth (20).
    async fn harness_with(config: ServerConfig, repository: Arc<MemoryRepository>) -> Harness {
        let service = AccessService::load(repository, Duration::from_secs(300), 4)
            .await
            .expect("service should load");
        let data_plane = Arc::new(RecordingDataPlane::default());
        let state = AppState {
            config: Arc::new(config),
            access: Arc::new(service),
            data_plane: data_plane.clone(),
            data_plane_ready: Arc::new(AtomicBool::new(true)),
            metrics: Arc::new(AppMetrics::default()),
        };
        Harness {
            app: build_router(state.clone()),
            state,
            data_plane,
        }
    }

    async fn harness() -> Harness {
        harness_with(two_protocol_config(), Arc::default()).await
    }

    fn two_protocol_config() -> ServerConfig {
        let mut config = hysteria_test_config();
        config.transports.push(stealth_profile());
        config
    }

    fn stealth_profile() -> TransportProfile {
        TransportProfile {
            id: "https-stealth-primary".to_owned(),
            display_name: "HTTPS Stealth".to_owned(),
            protocol: TransportProtocol::Stealth,
            endpoint: "vpn.example.test".to_owned(),
            port: 443,
            network: Network::Tcp,
            priority: 20,
            tls_server_name: Some("vpn.example.test".to_owned()),
            options: Default::default(),
        }
    }

    impl Harness {
        async fn send(&self, request: Request<Body>) -> Response {
            self.app
                .clone()
                .oneshot(request)
                .await
                .expect("request should complete")
        }

        async fn create(&self, body: Value) -> CreateAccessGrantResponse {
            let response = self
                .send(json_request(
                    "POST",
                    "/v1/admin/access-grants",
                    Some(ADMIN_TOKEN),
                    body,
                ))
                .await;
            assert_eq!(response.status(), StatusCode::CREATED);
            response_json(response).await
        }

        async fn enroll(&self, access_code: &str) -> Response {
            self.send(json_request(
                "POST",
                "/v1/sessions",
                None,
                json!({"access_code": access_code, "client_name": "Laptop"}),
            ))
            .await
        }

        async fn update(&self, id: &str, body: Value) -> Response {
            self.send(json_request(
                "PATCH",
                &format!("/v1/admin/access-grants/{id}"),
                Some(ADMIN_TOKEN),
                body,
            ))
            .await
        }

        async fn hysteria_accepts(&self, auth: &str) -> bool {
            let response = self
                .send(json_request(
                    "POST",
                    "/internal/v1/hysteria2/auth",
                    None,
                    json!({"addr": "192.0.2.10:12345", "auth": auth, "tx": 0}),
                ))
                .await;
            response_json::<HysteriaAuthResponse>(response).await.ok
        }
    }

    fn grant_body(protocols: Option<Value>) -> Value {
        let mut body = json!({"label": "Family", "max_connections": 2, "valid_for_minutes": 60});
        if let Some(protocols) = protocols {
            body["protocols"] = protocols;
        }
        body
    }

    async fn error_code(response: Response) -> String {
        response_json::<ErrorResponse>(response).await.code
    }

    #[tokio::test]
    async fn the_server_lists_the_protocols_it_offers_and_refuses_others() {
        let harness = harness().await;
        let unauthenticated = harness
            .send(empty_request("GET", "/v1/admin/protocols", None))
            .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let offered: Vec<OfferedProtocol> = response_json(
            harness
                .send(empty_request(
                    "GET",
                    "/v1/admin/protocols",
                    Some(ADMIN_TOKEN),
                ))
                .await,
        )
        .await;
        let names = offered
            .iter()
            .map(|protocol| (protocol.name.as_str(), protocol.display_name.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [("hysteria", "Hysteria 2"), ("stealth", "HTTPS Stealth")]
        );

        let refused = harness
            .send(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                grant_body(Some(json!({"only": ["wire_guard"]}))),
            ))
            .await;
        assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let refused: ErrorResponse = response_json(refused).await;
        assert_eq!(refused.message, "this server does not offer: wireguard");

        let empty = harness
            .send(json_request(
                "POST",
                "/v1/admin/access-grants",
                Some(ADMIN_TOKEN),
                grant_body(Some(json!({"only": []}))),
            ))
            .await;
        assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn a_protocol_limit_shapes_the_manifest_and_data_plane_access() {
        let harness = harness().await;
        let grant = harness
            .create(grant_body(Some(json!({"only": ["stealth"]}))))
            .await;
        assert_eq!(
            grant.protocols,
            ProtocolAccess::Only([TransportProtocol::Stealth].into())
        );
        let manifest: SessionManifest =
            response_json(harness.enroll(grant.access_code.expose_secret()).await).await;
        let protocols = manifest
            .transports
            .iter()
            .map(|transport| transport.profile.protocol.clone())
            .collect::<Vec<_>>();
        assert_eq!(protocols, [TransportProtocol::Stealth]);

        // Even a scoped token the manifest never carried cannot open Hysteria.
        let created = harness
            .state
            .access
            .create_session(grant.access_code.expose_secret(), "Phone".to_owned())
            .await
            .expect("second device should enroll");
        assert!(
            !harness
                .hysteria_accepts(created.data_plane_token.expose_secret())
                .await
        );

        let all = harness.create(grant_body(None)).await;
        let manifest: SessionManifest =
            response_json(harness.enroll(all.access_code.expose_secret()).await).await;
        assert_eq!(manifest.transports.len(), 2);
        let hysteria_auth = manifest.transports[0]
            .credential
            .as_ref()
            .expect("Hysteria credential")
            .auth
            .clone();
        assert!(
            harness
                .hysteria_accepts(hysteria_auth.expose_secret())
                .await
        );

        // Narrowing the protocols ends the session that holds Hysteria
        // credentials; the device then enrolls into the new set.
        let changed = harness
            .update(&all.id, json!({"protocols": {"only": ["stealth"]}}))
            .await;
        assert_eq!(changed.status(), StatusCode::OK);
        let changed: UpdateAccessGrantResponse = response_json(changed).await;
        assert_eq!(changed.disconnected_devices, 1);
        assert!(
            !harness
                .hysteria_accepts(hysteria_auth.expose_secret())
                .await
        );
        let manifest: SessionManifest =
            response_json(harness.enroll(all.access_code.expose_secret()).await).await;
        assert_eq!(manifest.transports.len(), 1);
    }

    #[tokio::test]
    async fn an_invite_limited_to_protocols_the_server_dropped_is_refused_clearly() {
        let repository = Arc::new(MemoryRepository::default());
        let before = harness_with(two_protocol_config(), repository.clone()).await;
        let grant = before
            .create(grant_body(Some(json!({"only": ["hysteria2"]}))))
            .await;

        let mut without_hysteria = test_config();
        without_hysteria.transports.push(stealth_profile());
        let after = harness_with(without_hysteria, repository).await;
        let refused = after.enroll(grant.access_code.expose_secret()).await;
        assert_eq!(refused.status(), StatusCode::CONFLICT);
        assert_eq!(error_code(refused).await, "grant_protocols_unavailable");
        assert_eq!(after.state.access.active_session_count().await, 0);
    }

    #[tokio::test]
    async fn pausing_disconnects_devices_and_refuses_them_until_resumed() {
        let harness = harness().await;
        let grant = harness.create(grant_body(None)).await;
        let manifest: SessionManifest =
            response_json(harness.enroll(grant.access_code.expose_secret()).await).await;
        let session_id = manifest.session_token.expose_secret()[..12].to_owned();
        let hysteria_auth = manifest.transports[0]
            .credential
            .as_ref()
            .expect("Hysteria credential")
            .auth
            .clone();

        let paused = harness.update(&grant.id, json!({"paused": true})).await;
        let paused: UpdateAccessGrantResponse = response_json(paused).await;
        assert!(paused.grant.paused);
        assert_eq!(paused.disconnected_devices, 1);
        assert!(
            harness
                .data_plane
                .kicked_sessions
                .lock()
                .unwrap()
                .contains(&session_id)
        );
        assert!(
            !harness
                .hysteria_accepts(hysteria_auth.expose_secret())
                .await
        );

        let refused = harness.enroll(grant.access_code.expose_secret()).await;
        assert_eq!(refused.status(), StatusCode::LOCKED);
        assert_eq!(error_code(refused).await, "grant_paused");

        let listed: Vec<AccessGrantSummary> = response_json(
            harness
                .send(empty_request(
                    "GET",
                    "/v1/admin/access-grants",
                    Some(ADMIN_TOKEN),
                ))
                .await,
        )
        .await;
        assert!(listed[0].paused);

        let resumed = harness.update(&grant.id, json!({"paused": false})).await;
        let resumed: UpdateAccessGrantResponse = response_json(resumed).await;
        assert!(!resumed.grant.paused);
        assert_eq!(resumed.disconnected_devices, 0);
        let enrolled = harness.enroll(grant.access_code.expose_secret()).await;
        assert_eq!(enrolled.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn details_show_connected_devices_and_edits_are_validated() {
        let harness = harness().await;
        let grant = harness.create(grant_body(None)).await;
        for _ in 0..2 {
            let enrolled = harness.enroll(grant.access_code.expose_secret()).await;
            assert_eq!(enrolled.status(), StatusCode::CREATED);
        }
        let path = format!("/v1/admin/access-grants/{}", grant.id);
        let details: AccessGrantDetails = response_json(
            harness
                .send(empty_request("GET", &path, Some(ADMIN_TOKEN)))
                .await,
        )
        .await;
        assert_eq!(details.grant.id, grant.id);
        assert_eq!(details.grant.active_connections, 2);
        assert_eq!(details.devices.len(), 2);
        assert!(
            details
                .devices
                .iter()
                .all(|device| device.client_name == "Laptop")
        );
        assert!(
            harness
                .send(empty_request("GET", &path, None))
                .await
                .status()
                == StatusCode::UNAUTHORIZED
        );
        let missing = harness
            .send(empty_request(
                "GET",
                "/v1/admin/access-grants/Missing001",
                Some(ADMIN_TOKEN),
            ))
            .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let new_end = unix_now() + 7 * 86_400;
        let edited = harness
            .update(
                &grant.id,
                json!({"label": "Cousins", "max_connections": 1, "expires_at_epoch_seconds": new_end}),
            )
            .await;
        assert_eq!(edited.status(), StatusCode::OK);
        let edited: UpdateAccessGrantResponse = response_json(edited).await;
        assert_eq!(edited.grant.label, "Cousins");
        assert_eq!(edited.grant.max_connections, 1);
        assert_eq!(edited.grant.expires_at_epoch_seconds, Some(new_end));
        assert_eq!(edited.disconnected_devices, 1);
        assert_eq!(edited.grant.active_connections, 1);

        for (body, status) in [
            (json!({}), StatusCode::UNPROCESSABLE_ENTITY),
            (
                json!({"expires_at_epoch_seconds": 1}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                json!({"expires_at_epoch_seconds": unix_now() + 400 * 86_400}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                json!({"max_connections": 0}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                json!({"max_connections": 5}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (json!({"label": "   "}), StatusCode::UNPROCESSABLE_ENTITY),
            (
                json!({"protocols": {"only": ["wire_guard"]}}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ] {
            assert_eq!(
                harness.update(&grant.id, body.clone()).await.status(),
                status,
                "{body}"
            );
        }
        assert_eq!(
            harness
                .update("Missing001", json!({"paused": true}))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}
