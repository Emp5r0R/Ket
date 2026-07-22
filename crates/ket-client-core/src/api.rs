use std::{net::IpAddr, time::Duration};

use async_trait::async_trait;
use ket_core::{CreateSessionRequest, ErrorResponse, SecretString, SessionManifest, SessionStatus};
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
        Ok(Self { base })
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
            .debug_tuple("ControlEndpoint")
            .field(&self.base.as_str())
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
        let response = request
            .send()
            .await
            .map_err(|error| ClientError::request(&error))?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if status != expected {
            return Err(api_error(status, &body));
        }
        serde_json::from_slice(&body)
            .map_err(|_| ClientError::InvalidResponse("JSON decoding failed".to_owned()))
    }

    async fn empty(
        &self,
        request: RequestBuilder,
        expected: StatusCode,
    ) -> Result<(), ClientError> {
        let response = request
            .send()
            .await
            .map_err(|error| ClientError::request(&error))?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if status == expected {
            Ok(())
        } else {
            Err(api_error(status, &body))
        }
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
        self.json(
            self.client
                .post(endpoint.route("v1/sessions")?)
                .json(&request),
            StatusCode::CREATED,
        )
        .await
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

fn api_error(status: StatusCode, body: &[u8]) -> ClientError {
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
