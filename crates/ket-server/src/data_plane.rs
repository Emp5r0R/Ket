use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, Uri, header};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use ket_core::SessionTraffic;
use serde::Deserialize;
use thiserror::Error;

use crate::{config::ServerConfig, service::unix_time};

const MAX_STATS_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub(crate) enum DataPlaneError {
    #[error("data-plane URL is invalid")]
    InvalidUrl,
    #[error("data-plane request could not be built: {0}")]
    Build(#[from] hyper::http::Error),
    #[error("data-plane request failed: {0}")]
    Request(#[from] hyper_util::client::legacy::Error),
    #[error("data-plane request timed out")]
    Timeout,
    #[error("data-plane response body failed: {0}")]
    Body(#[from] hyper::Error),
    #[error("data-plane returned HTTP {0}")]
    Status(StatusCode),
    #[error("data-plane returned too much data")]
    ResponseTooLarge,
    #[error("data-plane response is invalid: {0}")]
    Decode(#[from] serde_json::Error),
}

#[async_trait]
pub(crate) trait DataPlaneControl: Send + Sync {
    async fn healthy(&self) -> bool;
    async fn traffic(&self, session_id: &str) -> Result<SessionTraffic, DataPlaneError>;
    async fn kick(&self, session_ids: &[String]) -> Result<(), DataPlaneError>;
}

pub(crate) fn from_config(
    config: &ServerConfig,
) -> Result<Arc<dyn DataPlaneControl>, DataPlaneError> {
    match &config.hysteria {
        Some(hysteria) => Ok(Arc::new(HysteriaControl::new(
            &hysteria.stats_url,
            &hysteria.stats_secret,
        )?)),
        None => Ok(Arc::new(NoDataPlane)),
    }
}

struct NoDataPlane;

#[async_trait]
impl DataPlaneControl for NoDataPlane {
    async fn healthy(&self) -> bool {
        true
    }

    async fn traffic(&self, _session_id: &str) -> Result<SessionTraffic, DataPlaneError> {
        Ok(SessionTraffic {
            available: false,
            bytes_sent: 0,
            bytes_received: 0,
            online_connections: 0,
            observed_at_epoch_seconds: unix_time(),
        })
    }

    async fn kick(&self, _session_ids: &[String]) -> Result<(), DataPlaneError> {
        Ok(())
    }
}

type HttpClient = Client<HttpConnector, Full<Bytes>>;

struct HysteriaControl {
    client: HttpClient,
    base_url: String,
    secret: String,
}

impl HysteriaControl {
    fn new(base_url: &str, secret: &str) -> Result<Self, DataPlaneError> {
        let base_url = base_url.trim_end_matches('/').to_owned();
        let uri: Uri = base_url.parse().map_err(|_| DataPlaneError::InvalidUrl)?;
        if uri.scheme_str() != Some("http") || uri.authority().is_none() {
            return Err(DataPlaneError::InvalidUrl);
        }
        let mut connector = HttpConnector::new();
        connector.enforce_http(true);
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            base_url,
            secret: secret.to_owned(),
        })
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, DataPlaneError>
    where
        T: serde::de::DeserializeOwned,
    {
        let request = Request::builder()
            .method(Method::GET)
            .uri(self.uri(path)?)
            .header(header::AUTHORIZATION, &self.secret)
            .body(Full::new(Bytes::new()))?;
        self.send_json(request).await
    }

    async fn send_json<T>(&self, request: Request<Full<Bytes>>) -> Result<T, DataPlaneError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = tokio::time::timeout(Duration::from_secs(2), self.client.request(request))
            .await
            .map_err(|_| DataPlaneError::Timeout)??;
        if !response.status().is_success() {
            return Err(DataPlaneError::Status(response.status()));
        }
        let bytes = response.into_body().collect().await?.to_bytes();
        if bytes.len() > MAX_STATS_RESPONSE_BYTES {
            return Err(DataPlaneError::ResponseTooLarge);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn uri(&self, path: &str) -> Result<Uri, DataPlaneError> {
        format!("{}{path}", self.base_url)
            .parse()
            .map_err(|_| DataPlaneError::InvalidUrl)
    }
}

#[derive(Deserialize)]
struct HysteriaTraffic {
    tx: u64,
    rx: u64,
}

#[async_trait]
impl DataPlaneControl for HysteriaControl {
    async fn healthy(&self) -> bool {
        self.get_json::<BTreeMap<String, u32>>("/online")
            .await
            .is_ok()
    }

    async fn traffic(&self, session_id: &str) -> Result<SessionTraffic, DataPlaneError> {
        let (traffic, online) = tokio::join!(
            self.get_json::<BTreeMap<String, HysteriaTraffic>>("/traffic"),
            self.get_json::<BTreeMap<String, u32>>("/online")
        );
        let traffic = traffic?;
        let online = online?;
        let counters = traffic.get(session_id);
        Ok(SessionTraffic {
            available: true,
            bytes_sent: counters.map_or(0, |value| value.tx),
            bytes_received: counters.map_or(0, |value| value.rx),
            online_connections: online.get(session_id).copied().unwrap_or(0),
            observed_at_epoch_seconds: unix_time(),
        })
    }

    async fn kick(&self, session_ids: &[String]) -> Result<(), DataPlaneError> {
        if session_ids.is_empty() {
            return Ok(());
        }
        let body = serde_json::to_vec(session_ids)?;
        let request = Request::builder()
            .method(Method::POST)
            .uri(self.uri("/kick")?)
            .header(header::AUTHORIZATION, &self.secret)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))?;
        let response = tokio::time::timeout(Duration::from_secs(2), self.client.request(request))
            .await
            .map_err(|_| DataPlaneError::Timeout)??;
        if !response.status().is_success() {
            return Err(DataPlaneError::Status(response.status()));
        }
        Ok(())
    }
}
