use std::{net::IpAddr, time::Duration};

use async_trait::async_trait;
use ket_core::{CreateSessionRequest, ErrorResponse, SecretString, SessionManifest, SessionStatus};
use ket_invite::{
    ENROLLMENT_NONCE_HEADER, EnrollmentNonce, EnrollmentProof, NODE_SIGNATURE_HEADER, NodePublicKey,
};
use reqwest::{Client, RequestBuilder, StatusCode, Url, redirect::Policy, tls::Version};
use serde::de::DeserializeOwned;

use crate::ClientError;

const MAX_RESPONSE_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InsecureHttpPolicy {
    #[default]
    LoopbackOnly,
    Allow,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ControlEndpoint {
    base: Url,
    /// Pinned from an invite. When present, enrollment must be signed by it.
    node_key: Option<NodePublicKey>,
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
        })
    }

    /// Requires every enrollment through this endpoint to be signed by `key`.
    pub fn with_node_key(mut self, key: NodePublicKey) -> Self {
        self.node_key = Some(key);
        self
    }

    pub fn node_key(&self) -> Option<&NodePublicKey> {
        self.node_key.as_ref()
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
}

impl HttpControlPlane {
    pub fn new() -> Result<Self, ClientError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(Policy::none())
            .min_tls_version(Version::TLS_1_2)
            .no_proxy()
            .user_agent(concat!("Ket/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ClientError::Local("failed to initialize HTTP client".to_owned()))?;
        Ok(Self { client })
    }

    async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        expected: StatusCode,
    ) -> Result<T, ClientError> {
        let response = self.send(request, expected).await?;
        decode_json(&response.body)
    }

    /// Sends a request and returns the bounded body of an expected response;
    /// any other status becomes an API error.
    async fn send(
        &self,
        request: RequestBuilder,
        expected: StatusCode,
    ) -> Result<BoundedResponse, ClientError> {
        let response = request
            .send()
            .await
            .map_err(|error| ClientError::request(&error))?;
        let status = response.status();
        let node_signature = response
            .headers()
            .get(NODE_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = read_bounded(response).await?;
        if status != expected {
            return Err(api_error(status, &body));
        }
        Ok(BoundedResponse {
            body,
            node_signature,
        })
    }

    async fn empty(
        &self,
        request: RequestBuilder,
        expected: StatusCode,
    ) -> Result<(), ClientError> {
        self.send(request, expected).await.map(|_| ())
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
        let mut builder = self
            .client
            .post(endpoint.route("v1/sessions")?)
            .json(&request);
        let challenge = endpoint
            .node_key()
            .map(|key| (key, EnrollmentNonce::generate()));
        if let Some((_, nonce)) = &challenge {
            builder = builder.header(ENROLLMENT_NONCE_HEADER, nonce.header_value());
        }
        let response = self.send(builder, StatusCode::CREATED).await?;
        if let Some((key, nonce)) = &challenge {
            EnrollmentProof::verify(
                key,
                nonce,
                &response.body,
                response.node_signature.as_deref(),
            )
            .map_err(|_| ClientError::NodeIdentityMismatch)?;
        }
        decode_json(&response.body)
    }

    async fn session_status(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError> {
        self.json(
            self.client
                .get(endpoint.route("v1/sessions/current")?)
                .bearer_auth(token.expose_secret()),
            StatusCode::OK,
        )
        .await
    }

    async fn renew_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<SessionStatus, ClientError> {
        self.json(
            self.client
                .put(endpoint.route("v1/sessions/current")?)
                .bearer_auth(token.expose_secret()),
            StatusCode::OK,
        )
        .await
    }

    async fn release_session(
        &self,
        endpoint: &ControlEndpoint,
        token: &SecretString,
    ) -> Result<(), ClientError> {
        self.empty(
            self.client
                .delete(endpoint.route("v1/sessions/current")?)
                .bearer_auth(token.expose_secret()),
            StatusCode::NO_CONTENT,
        )
        .await
    }
}

struct BoundedResponse {
    body: Vec<u8>,
    node_signature: Option<String>,
}

fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, ClientError> {
    serde_json::from_slice(body)
        .map_err(|_| ClientError::InvalidResponse("JSON decoding failed".to_owned()))
}

async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>, ClientError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ClientError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ClientError::request(&error))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
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
fn api_error(status: StatusCode, body: &[u8]) -> ClientError {
    if status == StatusCode::NOT_FOUND {
        return ClientError::Api {
            status: status.as_u16(),
            code: "unauthorized".to_owned(),
            message: "the server address or access code was not accepted".to_owned(),
        };
    }
    let response = serde_json::from_slice::<ErrorResponse>(body).ok();
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
            "/oversized",
            get(|| async { vec![b'x'; MAX_RESPONSE_BYTES + 1] }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let endpoint = ControlEndpoint::parse(&format!("http://{address}")).unwrap();
        let api = HttpControlPlane::new().unwrap();
        let result = api
            .json::<serde_json::Value>(
                api.client.get(endpoint.route("oversized").unwrap()),
                StatusCode::OK,
            )
            .await;
        assert!(matches!(result, Err(ClientError::ResponseTooLarge)));
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
        }
    }
}
