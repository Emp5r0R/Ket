use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use ket_core::{
    CLIENT_HEADER, CLIENT_UPDATE_REQUIRED_CODE, ClientUpdate, CreateSessionRequest, ErrorResponse,
    SEALED_CONTROL_PATH, SealedMethod, SecretString, SessionManifest, SessionStatus, UpdateAdvice,
};
use ket_invite::{
    ENROLLMENT_NONCE_HEADER, EnrollmentNonce, EnrollmentProof, NODE_SIGNATURE_HEADER, NodePublicKey,
};
use reqwest::{Client, Method, StatusCode, Url, redirect::Policy, tls::Version};
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

use crate::{ClientError, sealed_control::SealedControl};

const MAX_RESPONSE_BYTES: usize = 128 * 1024;

/// This app's `Ket-Client` header value, for example `linux/1.2.0`. The
/// server uses it to decide whether this version must, should, or may update.
pub fn client_header_value() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some(concat!("linux/", env!("CARGO_PKG_VERSION")))
    } else if cfg!(target_os = "windows") {
        Some(concat!("windows/", env!("CARGO_PKG_VERSION")))
    } else if cfg!(target_os = "macos") {
        Some(concat!("macos/", env!("CARGO_PKG_VERSION")))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InsecureHttpPolicy {
    #[default]
    LoopbackOnly,
    Allow,
}

/// Whether session API requests to an endpoint are sealed to its node key.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Sealing {
    /// No pinned node key: URL + access code clients.
    Off,
    /// A pinned key from an invite that predates sealed control. Seal when
    /// the server offers it, otherwise enroll in the clear with a signed
    /// response.
    Preferred,
    /// The invite offered sealed control, so nothing goes in the clear.
    Required,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ControlEndpoint {
    base: Url,
    /// Pinned from an invite. When present, enrollment must be signed by it.
    node_key: Option<NodePublicKey>,
    sealing: Sealing,
}

impl ControlEndpoint {
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        Self::parse_with_policy(value, InsecureHttpPolicy::LoopbackOnly)
    }

    pub fn parse_with_policy(
        value: &str,
        insecure_http: InsecureHttpPolicy,
    ) -> Result<Self, ClientError> {
        let mut base = Url::parse(value.trim())
            .map_err(|_| ClientError::InvalidEndpoint("expected an absolute URL".to_owned()))?;
        if !base.username().is_empty() || base.password().is_some() {
            return Err(ClientError::InvalidEndpoint(
                "embedded credentials are not allowed".to_owned(),
            ));
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(ClientError::InvalidEndpoint(
                "query strings and fragments are not allowed".to_owned(),
            ));
        }
        let host = base
            .host_str()
            .ok_or_else(|| ClientError::InvalidEndpoint("a host is required".to_owned()))?;
        match base.scheme() {
            "https" => {}
            "http" if insecure_http == InsecureHttpPolicy::Allow || is_loopback_host(host) => {}
            "http" => {
                return Err(ClientError::InvalidEndpoint(
                    "unencrypted HTTP is allowed only for loopback development servers".to_owned(),
                ));
            }
            _ => {
                return Err(ClientError::InvalidEndpoint(
                    "only https:// and loopback http:// URLs are supported".to_owned(),
                ));
            }
        }

        let normalized_path = format!("{}/", base.path().trim_end_matches('/'));
        base.set_path(&normalized_path);
        Ok(Self {
            base,
            node_key: None,
            sealing: Sealing::Off,
        })
    }

    /// Requires every enrollment through this endpoint to be signed by `key`,
    /// and seals requests to it when the server offers that.
    pub fn with_node_key(mut self, key: NodePublicKey) -> Self {
        self.node_key = Some(key);
        self.sealing = self.sealing.max(Sealing::Preferred);
        self
    }

    /// Never sends a session API request in the clear. Only meaningful with
    /// a node key; without one, nothing can be sealed and requests fail.
    pub fn with_required_sealing(mut self, required: bool) -> Self {
        if required {
            self.sealing = Sealing::Required;
        }
        self
    }

    pub fn sealing(&self) -> Sealing {
        self.sealing
    }

    /// Whether saved profiles must keep sealing required.
    pub fn sealing_required(&self) -> bool {
        self.sealing == Sealing::Required
    }

    /// Parses a saved profile: the URL plus the optional pinned node key in
    /// its persisted Base64URL form.
    pub fn parse_pinned(value: &str, node_key: Option<&str>) -> Result<Self, ClientError> {
        let endpoint = Self::parse(value)?;
        match node_key {
            None => Ok(endpoint),
            Some(text) => NodePublicKey::from_base64url(text)
                .map(|key| endpoint.with_node_key(key))
                .map_err(ClientError::Invite),
        }
    }

    pub fn node_key(&self) -> Option<&NodePublicKey> {
        self.node_key.as_ref()
    }

    /// The pinned node key in its persisted Base64URL form.
    pub fn node_key_text(&self) -> Option<String> {
        self.node_key.as_ref().map(NodePublicKey::to_base64url)
    }

    pub fn as_str(&self) -> &str {
        self.base.as_str()
    }

    fn route(&self, relative: &str) -> Result<Url, ClientError> {
        self.base
            .join(relative.trim_start_matches('/'))
            .map_err(|_| {
                ClientError::InvalidEndpoint("failed to construct an API route".to_owned())
            })
    }
}

impl std::fmt::Debug for ControlEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlEndpoint")
            .field("base", &self.base.as_str())
            .field("node_key", &self.node_key)
            .field("sealing", &self.sealing)
            .finish()
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[async_trait]
pub trait ControlPlane: Send + Sync {
    async fn create_session(
        &self,
        endpoint: &ControlEndpoint,
        access_code: &SecretString,
        client_name: &str,
    ) -> Result<SessionManifest, ClientError>;

    async fn session_status(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError>;

    async fn renew_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError>;

    async fn release_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<(), ClientError>;
}

#[derive(Clone)]
pub struct HttpControlPlane {
    client: Client,
    sealed: Arc<SealedControl>,
}

/// One session API call, independent of whether it travels sealed.
pub(crate) struct ControlRequest<'a> {
    pub(crate) method: SealedMethod,
    /// Relative to the endpoint, such as `v1/sessions`.
    pub(crate) path: &'static str,
    pub(crate) bearer: Option<&'a SecretString>,
    /// JSON, for enrollment.
    pub(crate) body: Option<Zeroizing<String>>,
}

/// The session API's answer, sealed or not.
pub(crate) struct Exchange {
    pub(crate) status: StatusCode,
    pub(crate) body: Vec<u8>,
}

impl Exchange {
    fn expect(self, expected: StatusCode) -> Result<Vec<u8>, ClientError> {
        if self.status == expected {
            Ok(self.body)
        } else {
            Err(api_error(self.status, &self.body))
        }
    }
}

impl HttpControlPlane {
    pub fn new() -> Result<Self, ClientError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(Policy::none())
            .min_tls_version(Version::TLS_1_2)
            .no_proxy()
            // A fresh connection per call: control calls are rare, and a kept
            // connection can die silently inside the tunnel when the device
            // changes networks, so the next call would hang until it timed out.
            .pool_max_idle_per_host(0)
            // No version: sealed requests must not tell a front which app
            // version is asking. Plain requests name it in `Ket-Client`.
            .user_agent("Ket")
            .build()
            .map_err(|_| ClientError::Local("failed to initialize HTTP client".to_owned()))?;
        Ok(Self {
            client,
            sealed: Arc::new(SealedControl::default()),
        })
    }

    /// Sends `request` sealed when the endpoint allows, otherwise in the
    /// clear. Either way a successful enrollment is proven to come from the
    /// pinned node: by the seal, or by the node's signature.
    async fn exchange(
        &self,
        endpoint: &ControlEndpoint,
        request: ControlRequest<'_>,
    ) -> Result<Exchange, ClientError> {
        if let (Some(node_key), Sealing::Preferred | Sealing::Required) =
            (endpoint.node_key(), endpoint.sealing())
        {
            let url = endpoint.route(SEALED_CONTROL_PATH)?;
            match self
                .sealed
                .exchange(&self.client, url, node_key, &request)
                .await?
            {
                Some(exchange) => return Ok(exchange),
                None if endpoint.sealing() == Sealing::Preferred => {}
                None => return Err(ClientError::SealedControlUnavailable),
            }
        } else if endpoint.sealing() == Sealing::Required {
            return Err(ClientError::SealedControlUnavailable);
        }
        self.plain_exchange(endpoint, request).await
    }

    async fn plain_exchange(
        &self,
        endpoint: &ControlEndpoint,
        request: ControlRequest<'_>,
    ) -> Result<Exchange, ClientError> {
        let method = match request.method {
            SealedMethod::Get => Method::GET,
            SealedMethod::Post => Method::POST,
            SealedMethod::Put => Method::PUT,
            SealedMethod::Delete => Method::DELETE,
        };
        let mut builder = self.client.request(method, endpoint.route(request.path)?);
        if let Some(value) = client_header_value() {
            builder = builder.header(CLIENT_HEADER, value);
        }
        if let Some(token) = request.bearer {
            builder = builder.bearer_auth(token.expose_secret());
        }
        let challenge = request.body.as_ref().and(
            endpoint
                .node_key()
                .map(|key| (key, EnrollmentNonce::generate())),
        );
        if let Some((_, nonce)) = &challenge {
            builder = builder.header(ENROLLMENT_NONCE_HEADER, nonce.header_value());
        }
        if let Some(body) = &request.body {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.as_str().to_owned());
        }
        let response = builder
            .send()
            .await
            .map_err(|error| ClientError::request(&error))?;
        let status = response.status();
        let node_signature = response
            .headers()
            .get(NODE_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = read_bounded(response, MAX_RESPONSE_BYTES).await?;
        if let Some((key, nonce)) = &challenge
            && status.is_success()
        {
            EnrollmentProof::verify(key, nonce, &body, node_signature.as_deref())
                .map_err(|_| ClientError::NodeIdentityMismatch)?;
        }
        Ok(Exchange { status, body })
    }

    async fn status_call(
        &self,
        endpoint: &ControlEndpoint,
        method: SealedMethod,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError> {
        let body = self
            .exchange(
                endpoint,
                ControlRequest {
                    method,
                    path: "v1/sessions/current",
                    bearer: Some(token),
                    body: None,
                },
            )
            .await?
            .expect(StatusCode::OK)?;
        let mut status: SessionStatus = decode_json(&body)?;
        status.updates = sanitize_advice(status.updates.take());
        Ok(status)
    }
}

impl Default for HttpControlPlane {
    fn default() -> Self {
        Self::new().expect("the static HTTP client configuration must be valid")
    }
}

#[async_trait]
impl ControlPlane for HttpControlPlane {
    async fn create_session(
        &self,
        endpoint: &ControlEndpoint,
        access_code: &SecretString,
        client_name: &str,
    ) -> Result<SessionManifest, ClientError> {
        let request = CreateSessionRequest {
            access_code: access_code.clone(),
            client_name: client_name.to_owned(),
        };
        let body = Zeroizing::new(
            serde_json::to_string(&request)
                .map_err(|_| ClientError::Local("failed to encode a request".to_owned()))?,
        );
        let response = self
            .exchange(
                endpoint,
                ControlRequest {
                    method: SealedMethod::Post,
                    path: "v1/sessions",
                    bearer: None,
                    body: Some(body),
                },
            )
            .await?
            .expect(StatusCode::CREATED)?;
        let mut manifest: SessionManifest = decode_json(&response)?;
        manifest.updates = sanitize_advice(manifest.updates.take());
        Ok(manifest)
    }

    async fn session_status(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError> {
        self.status_call(endpoint, SealedMethod::Get, token).await
    }

    async fn renew_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError> {
        self.status_call(endpoint, SealedMethod::Put, token).await
    }

    async fn release_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<(), ClientError> {
        self.exchange(
            endpoint,
            ControlRequest {
                method: SealedMethod::Delete,
                path: "v1/sessions/current",
                bearer: Some(token),
                body: None,
            },
        )
        .await?
        .expect(StatusCode::NO_CONTENT)
        .map(|_| ())
    }
}

fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, ClientError> {
    serde_json::from_slice(body)
        .map_err(|_| ClientError::InvalidResponse("JSON decoding failed".to_owned()))
}

pub(crate) async fn read_bounded(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ClientError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ClientError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ClientError::request(&error))?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ClientError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Maps a non-success control response to a client error.
///
/// The public edge answers rejected credentials with the same neutral `404`
/// page it serves to scanners, so on the session routes a `404` means the
/// access code or session token was not accepted.
pub(crate) fn api_error(status: StatusCode, body: &[u8]) -> ClientError {
    if status == StatusCode::NOT_FOUND {
        return ClientError::Api {
            status: status.as_u16(),
            code: "unauthorized".to_owned(),
            message: "the server address or access code was not accepted".to_owned(),
        };
    }
    let response = serde_json::from_slice::<ErrorResponse>(body).ok();
    if status == StatusCode::UPGRADE_REQUIRED
        && let Some(update) = response
            .as_ref()
            .filter(|response| response.code == CLIENT_UPDATE_REQUIRED_CODE)
            .and_then(|response| response.update.clone())
            .and_then(sanitize_update)
    {
        return ClientError::UpdateRequired(Box::new(update));
    }
    let code = response
        .as_ref()
        .map(|response| response.code.as_str())
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("http_{}", status.as_u16()));
    let message = response
        .map(|response| sanitize_message(&response.message))
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "the server rejected the request".to_owned());
    ClientError::Api {
        status: status.as_u16(),
        code,
        message,
    }
}

/// Keeps server-supplied update details displayable and safe to open: the
/// summary is bounded text and the download page must be HTTPS.
fn sanitize_update(mut update: ClientUpdate) -> Option<ClientUpdate> {
    update.summary = update
        .summary
        .map(|summary| sanitize_message(&summary))
        .filter(|summary| !summary.is_empty());
    update.latest_version = sanitize_message(&update.latest_version)
        .chars()
        .take(32)
        .collect();
    let url = reqwest::Url::parse(&update.download_url).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && update.download_url.len() <= 2_048)
        .then_some(update)
}

/// Applies [`sanitize_update`] to the advice in a successful response; an
/// unusable download page drops the advice rather than the session.
pub(crate) fn sanitize_advice(advice: Option<UpdateAdvice>) -> Option<UpdateAdvice> {
    let mut advice = advice?;
    advice.server_version = sanitize_message(&advice.server_version)
        .chars()
        .take(32)
        .collect();
    advice.client = sanitize_update(advice.client)?;
    Some(advice)
}

fn sanitize_message(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(256)
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::State,
        routing::{delete, get, post},
    };
    use ket_core::{HealthState, NodeLocation, NodeStatus, generate_session_token};

    use super::*;

    #[test]
    fn endpoint_policy_requires_tls_outside_loopback() {
        assert!(ControlEndpoint::parse("https://ket.example.com").is_ok());
        assert!(ControlEndpoint::parse("http://127.0.0.1:8787").is_ok());
        assert!(ControlEndpoint::parse("http://[::1]:8787").is_ok());
        assert!(ControlEndpoint::parse("http://ket.example.com").is_err());
        assert!(ControlEndpoint::parse("https://user:pass@ket.example.com").is_err());
        assert!(ControlEndpoint::parse("https://ket.example.com?token=nope").is_err());
    }

    #[test]
    fn saved_profiles_restore_the_pinned_node_key() {
        let key = ket_invite::NodeSigningKey::from_seed(&[8; 32]).public_key();
        let text = key.to_base64url();
        let pinned = ControlEndpoint::parse_pinned("https://ket.example.com", Some(&text)).unwrap();
        assert_eq!(pinned.node_key(), Some(&key));
        assert_eq!(pinned.node_key_text(), Some(text));
        let plain = ControlEndpoint::parse_pinned("https://ket.example.com", None).unwrap();
        assert_eq!(plain.node_key(), None);
        let error =
            ControlEndpoint::parse_pinned("https://ket.example.com", Some("bad key")).unwrap_err();
        assert_eq!(error.issue().code, "invalid_node_key");
    }

    #[test]
    fn endpoint_preserves_a_reverse_proxy_path_prefix() {
        let endpoint = ControlEndpoint::parse("https://example.com/ket").unwrap();
        assert_eq!(
            endpoint.route("v1/sessions").unwrap().as_str(),
            "https://example.com/ket/v1/sessions"
        );
    }

    #[test]
    fn api_errors_are_bounded_before_reaching_the_ui() {
        let body = serde_json::to_vec(&ErrorResponse {
            code: "INVALID CODE".to_owned(),
            message: format!("bad\n{}", "x".repeat(400)),
            update: None,
        })
        .unwrap();
        let ClientError::Api { code, message, .. } = api_error(StatusCode::BAD_REQUEST, &body)
        else {
            panic!("expected API error");
        };
        assert_eq!(code, "http_400");
        assert!(!message.contains('\n'));
        assert_eq!(message.chars().count(), 256);
    }

    #[test]
    fn masked_edge_rejection_is_reported_as_unauthorized() {
        let decoy = b"<!doctype html><html><body><p>Not Found</p></body></html>\n";
        let error = api_error(StatusCode::NOT_FOUND, decoy);
        assert!(error.is_unauthorized());
        let ClientError::Api {
            status,
            code,
            message,
        } = error
        else {
            panic!("expected API error");
        };
        assert_eq!(status, 404);
        assert_eq!(code, "unauthorized");
        assert_eq!(
            message,
            "the server address or access code was not accepted"
        );
        assert!(!message.contains("Not Found"));
    }

    #[tokio::test]
    async fn http_client_exchanges_and_releases_a_real_loopback_session() {
        let manifest = test_manifest();
        let app = Router::new()
            .route(
                "/v1/sessions",
                post(
                    |State(manifest): State<SessionManifest>,
                     Json(request): Json<CreateSessionRequest>| async move {
                        assert_eq!(request.access_code.len(), 32);
                        assert_eq!(request.client_name, "Integration client");
                        (StatusCode::CREATED, Json(manifest))
                    },
                ),
            )
            .route(
                "/v1/sessions/current",
                delete(|| async { StatusCode::NO_CONTENT }),
            )
            .with_state(manifest.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let endpoint = ControlEndpoint::parse(&format!("http://{address}")).unwrap();
        let api = HttpControlPlane::new().unwrap();

        let created = api
            .create_session(
                &endpoint,
                &SecretString::from("A2345678901234567890123456789012"),
                "Integration client",
            )
            .await
            .unwrap();
        assert_eq!(created.session_token, manifest.session_token);
        api.release_session(&endpoint, &created.session_token)
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn http_client_names_itself_and_reports_required_updates() {
        use axum::http::HeaderMap;
        let app = Router::new().route(
            "/v1/sessions",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers
                        .get(CLIENT_HEADER)
                        .and_then(|value| value.to_str().ok()),
                    client_header_value()
                );
                (
                    StatusCode::UPGRADE_REQUIRED,
                    Json(serde_json::json!({
                        "code": "client_update_required",
                        "message": "update Ket",
                        "update": {
                            "status": "required",
                            "latest_version": "9.0.0",
                            "summary": "Critical\nfix",
                            "download_url": "https://mirror.example.test/ket"
                        }
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let endpoint = ControlEndpoint::parse(&format!("http://{address}")).unwrap();
        let error = HttpControlPlane::new()
            .unwrap()
            .create_session(
                &endpoint,
                &SecretString::from("A2345678901234567890123456789012"),
                "Integration client",
            )
            .await
            .unwrap_err();
        assert!(error.ends_session());
        assert!(!error.is_unauthorized());
        assert_eq!(error.issue().code, "client_update_required");
        let update = error.required_update().expect("update details");
        assert_eq!(update.latest_version, "9.0.0");
        assert_eq!(update.summary.as_deref(), Some("Criticalfix"));
        server.abort();
    }

    #[test]
    fn update_details_with_unsafe_download_pages_are_dropped() {
        let body = |url: &str| {
            serde_json::to_vec(&serde_json::json!({
                "code": "client_update_required",
                "message": "update Ket",
                "update": {"status": "required", "latest_version": "9.0.0", "download_url": url}
            }))
            .unwrap()
        };
        for url in [
            "http://mirror.example.test/",
            "javascript:alert(1)",
            "https://user:secret@mirror.example.test/",
        ] {
            let error = api_error(StatusCode::UPGRADE_REQUIRED, &body(url));
            assert!(error.required_update().is_none(), "{url}");
            assert!(!error.ends_session(), "{url}");
        }
        // A 426 without the update code is an ordinary API error.
        let error = api_error(StatusCode::UPGRADE_REQUIRED, b"{}");
        assert!(error.required_update().is_none());
        assert!(
            sanitize_advice(Some(UpdateAdvice {
                server_version: "9.0.0".to_owned(),
                client: ClientUpdate {
                    status: ket_core::ClientUpdateStatus::Optional,
                    latest_version: "9.0.0".to_owned(),
                    required_after_epoch_seconds: None,
                    summary: None,
                    download_url: "ftp://mirror.example.test/".to_owned(),
                },
            }))
            .is_none()
        );
    }

    /// Serves enrollment the way a signing node does, optionally signing with
    /// a different key or omitting the signature, and records the nonce.
    async fn signing_server(
        signer_seed: Option<[u8; 32]>,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<()>,
        std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
    ) {
        use axum::{http::HeaderMap, response::IntoResponse};
        let nonces = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = nonces.clone();
        let manifest = test_manifest();
        let app = Router::new().route(
            "/v1/sessions",
            post(move |headers: HeaderMap| {
                let manifest = manifest.clone();
                let recorded = recorded.clone();
                async move {
                    let nonce = headers
                        .get(ENROLLMENT_NONCE_HEADER)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    recorded.lock().unwrap().push(nonce.clone());
                    let body = serde_json::to_vec(&manifest).unwrap();
                    let mut response = (StatusCode::CREATED, body.clone()).into_response();
                    if let (Some(seed), Some(nonce)) = (signer_seed, nonce) {
                        let nonce = EnrollmentNonce::from_header_value(&nonce).unwrap();
                        let key = ket_invite::NodeSigningKey::from_seed(&seed);
                        response.headers_mut().insert(
                            NODE_SIGNATURE_HEADER,
                            EnrollmentProof::sign(&key, &nonce, &body).parse().unwrap(),
                        );
                    }
                    response
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server, nonces)
    }

    #[tokio::test]
    async fn pinned_endpoints_require_the_node_signature_over_the_manifest() {
        const NODE_SEED: [u8; 32] = [0x77; 32];
        let node_key = ket_invite::NodeSigningKey::from_seed(&NODE_SEED).public_key();
        let api = HttpControlPlane::new().unwrap();
        let code = SecretString::from("A2345678901234567890123456789012");

        let (address, server, nonces) = signing_server(Some(NODE_SEED)).await;
        let pinned = ControlEndpoint::parse(&format!("http://{address}"))
            .unwrap()
            .with_node_key(node_key);
        api.create_session(&pinned, &code, "Pinned client")
            .await
            .expect("a correctly signed manifest is accepted");
        let unpinned = ControlEndpoint::parse(&format!("http://{address}")).unwrap();
        api.create_session(&unpinned, &code, "Plain client")
            .await
            .expect("URL + access code enrollment is unchanged");
        let recorded = nonces.lock().unwrap().clone();
        assert!(
            recorded[0]
                .as_deref()
                .is_some_and(|nonce| nonce.len() == 43)
        );
        assert_eq!(recorded[1], None, "unpinned clients send no nonce");
        server.abort();

        for signer in [None, Some([0x78; 32])] {
            let (address, server, _) = signing_server(signer).await;
            let pinned = ControlEndpoint::parse(&format!("http://{address}"))
                .unwrap()
                .with_node_key(node_key);
            let result = api.create_session(&pinned, &code, "Pinned client").await;
            assert!(
                matches!(result, Err(ClientError::NodeIdentityMismatch)),
                "signer {signer:?}"
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn http_client_stops_reading_oversized_responses() {
        let app = Router::new().route(
            "/v1/sessions/current",
            get(|| async { vec![b'x'; MAX_RESPONSE_BYTES + 1] }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let endpoint = ControlEndpoint::parse(&format!("http://{address}")).unwrap();
        let result = HttpControlPlane::new()
            .unwrap()
            .session_status(&endpoint, &SecretString::from("token"))
            .await;
        assert!(matches!(result, Err(ClientError::ResponseTooLarge)));
        server.abort();
    }

    const SEALED_NODE_SEED: [u8; 32] = [0x79; 32];

    #[derive(Clone, Default)]
    struct SealedNode {
        /// Opened requests, in order.
        seen: std::sync::Arc<std::sync::Mutex<Vec<ket_core::SealedControlRequest>>>,
        /// Plain requests, which a sealing client must never send.
        plain: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        /// Seconds the node's clock is ahead of the device's.
        clock_ahead: u64,
        /// Answer every sealed request with 404, as an older server does.
        no_sealed_route: bool,
    }

    /// A node that answers the session API sealed (and in the clear, counting
    /// those requests), enforcing the clock window like the real gateway.
    async fn sealed_node(node: SealedNode) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        use axum::{body::Bytes, response::IntoResponse};
        let opening = std::sync::Arc::new(ket_seal::NodeOpeningKey::from_ed25519_seed(
            &SEALED_NODE_SEED,
        ));
        let manifest = test_manifest();
        let sealed_state = node.clone();
        let plain_state = node.clone();
        let plain_manifest = manifest.clone();
        let app = Router::new()
            .route(
                "/v1/sealed",
                post(move |body: Bytes| {
                    let node = sealed_state.clone();
                    let manifest = manifest.clone();
                    let opened = ket_seal::open_request(&opening, &body);

                    async move {
                        if node.no_sealed_route {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        let Ok(opened) = opened else {
                            return StatusCode::NOT_FOUND.into_response();
                        };
                        let request: ket_core::SealedControlRequest =
                            serde_json::from_slice(&opened.plaintext).unwrap();
                        node.seen.lock().unwrap().push(request.clone());
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                            + node.clock_ahead;
                        let (status, body) = if request.sent_at_epoch_seconds.abs_diff(now)
                            > ket_core::SEALED_MAX_CLOCK_SKEW_SECONDS
                        {
                            (
                                400,
                                Some(
                                    serde_json::json!({
                                        "code": ket_core::SEALED_CLOCK_SKEW_CODE,
                                        "message": "clock"
                                    })
                                    .to_string(),
                                ),
                            )
                        } else {
                            match (request.method, request.path.as_str()) {
                                (SealedMethod::Post, "/v1/sessions") => {
                                    (201, Some(serde_json::to_string(&manifest).unwrap()))
                                }
                                (SealedMethod::Delete, "/v1/sessions/current") => (204, None),
                                _ => (404, None),
                            }
                        };
                        let answer = ket_core::SealedControlResponse {
                            status,
                            server_time_epoch_seconds: now,
                            body: body.map(SecretString::new),
                        };
                        let sealed = opened
                            .sealer
                            .seal(&serde_json::to_vec(&answer).unwrap())
                            .unwrap();
                        (StatusCode::OK, sealed).into_response()
                    }
                }),
            )
            .route(
                "/v1/sessions",
                post(move || {
                    plain_state
                        .plain
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let manifest = plain_manifest.clone();
                    async move { (StatusCode::CREATED, Json(manifest)) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server)
    }

    fn pinned_endpoint(address: std::net::SocketAddr, required: bool) -> ControlEndpoint {
        ControlEndpoint::parse(&format!("http://{address}"))
            .unwrap()
            .with_node_key(ket_invite::NodeSigningKey::from_seed(&SEALED_NODE_SEED).public_key())
            .with_required_sealing(required)
    }

    #[tokio::test]
    async fn pinned_endpoints_seal_every_session_request() {
        let node = SealedNode::default();
        let (address, server) = sealed_node(node.clone()).await;
        let endpoint = pinned_endpoint(address, true);
        let api = HttpControlPlane::new().unwrap();
        let code = SecretString::from("A2345678901234567890123456789012");
        let manifest = api
            .create_session(&endpoint, &code, "Sealed client")
            .await
            .unwrap();
        assert_eq!(manifest.node.node_id, "test-node");
        api.release_session(&endpoint, &manifest.session_token)
            .await
            .unwrap();
        let seen = node.seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].path, "/v1/sessions");
        let body: CreateSessionRequest =
            serde_json::from_str(seen[0].body.as_ref().unwrap().expose_secret()).unwrap();
        assert_eq!(body.access_code, code);
        assert_eq!(seen[0].client.as_deref(), client_header_value());
        assert_eq!(seen[1].method, SealedMethod::Delete);
        assert_eq!(seen[1].bearer.as_ref(), Some(&manifest.session_token));
        assert_eq!(node.plain.load(std::sync::atomic::Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn a_skewed_device_clock_is_corrected_once_from_the_sealed_answer() {
        let node = SealedNode {
            clock_ahead: 3_600,
            ..SealedNode::default()
        };
        let (address, server) = sealed_node(node.clone()).await;
        let endpoint = pinned_endpoint(address, true);
        let api = HttpControlPlane::new().unwrap();
        let code = SecretString::from("A2345678901234567890123456789012");
        api.create_session(&endpoint, &code, "Skewed client")
            .await
            .unwrap();
        api.create_session(&endpoint, &code, "Skewed client")
            .await
            .unwrap();
        assert_eq!(
            node.seen.lock().unwrap().len(),
            3,
            "one refused attempt, then both calls use the corrected clock"
        );
        server.abort();
    }

    #[tokio::test]
    async fn required_sealing_never_falls_back_to_the_clear() {
        let node = SealedNode {
            no_sealed_route: true,
            ..SealedNode::default()
        };
        let (address, server) = sealed_node(node.clone()).await;
        let api = HttpControlPlane::new().unwrap();
        let code = SecretString::from("A2345678901234567890123456789012");
        let result = api
            .create_session(&pinned_endpoint(address, true), &code, "Client")
            .await;
        assert!(matches!(result, Err(ClientError::SealedControlUnavailable)));
        assert_eq!(node.plain.load(std::sync::atomic::Ordering::SeqCst), 0);

        // An endpoint from an invite that predates sealed control may still
        // enroll in the clear, but must then get the node's signature.
        let result = api
            .create_session(&pinned_endpoint(address, false), &code, "Client")
            .await;
        assert!(matches!(result, Err(ClientError::NodeIdentityMismatch)));
        assert_eq!(node.plain.load(std::sync::atomic::Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn an_answer_sealed_by_anyone_but_the_pinned_node_is_refused() {
        let (address, server) = sealed_node(SealedNode::default()).await;
        let endpoint = ControlEndpoint::parse(&format!("http://{address}"))
            .unwrap()
            .with_node_key(ket_invite::NodeSigningKey::from_seed(&[0x7a; 32]).public_key())
            .with_required_sealing(true);
        // The node cannot open a request sealed to another key and answers
        // 404, so nothing is sent in the clear and nothing is accepted.
        let result = HttpControlPlane::new()
            .unwrap()
            .create_session(
                &endpoint,
                &SecretString::from("A2345678901234567890123456789012"),
                "Client",
            )
            .await;
        assert!(matches!(result, Err(ClientError::SealedControlUnavailable)));
        server.abort();
    }

    fn test_manifest() -> SessionManifest {
        SessionManifest {
            session_token: generate_session_token().into(),
            session_expires_at_epoch_seconds: 4_000_000_000,
            access_expires_at_epoch_seconds: Some(4_000_003_600),
            node: NodeStatus {
                node_id: "test-node".to_owned(),
                display_name: "Test node".to_owned(),
                public_url: "http://127.0.0.1".to_owned(),
                location: NodeLocation {
                    country_code: "IN".to_owned(),
                    country_name: "India".to_owned(),
                    city: Some("Hyderabad".to_owned()),
                    latitude: 17.385,
                    longitude: 78.4867,
                },
                health: HealthState::Healthy,
                active_sessions: 1,
                session_capacity: 10,
                capacity_percent: 10.0,
                cpu_load_percent: None,
                memory_used_bytes: None,
                memory_total_bytes: None,
                uptime_seconds: None,
                observed_at_epoch_seconds: 3_000_000_000,
            },
            transports: Vec::new(),
            updates: None,
        }
    }
}
