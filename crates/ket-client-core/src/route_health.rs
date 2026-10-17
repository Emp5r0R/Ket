use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy, tls::Version};

use crate::ClientError;

const GOOGLE_CONNECTIVITY_CHECK: &str = "https://connectivitycheck.gstatic.com/generate_204";
const CLOUDFLARE_CONNECTIVITY_CHECK: &str = "https://cp.cloudflare.com/generate_204";

#[async_trait]
pub(crate) trait RouteHealthProbe: Send + Sync {
    async fn is_healthy(&self) -> bool;
}

pub(crate) struct HttpsRouteHealthProbe {
    client: Client,
}

impl HttpsRouteHealthProbe {
    pub(crate) fn new() -> Result<Self, ClientError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(6))
            .redirect(Policy::none())
            .min_tls_version(Version::TLS_1_2)
            .no_proxy()
            // Each check needs a fresh path through the tunnel; a kept
            // connection from before a network change may be dead.
            .pool_max_idle_per_host(0)
            .user_agent(concat!("Ket/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                ClientError::Local("failed to initialize the route health probe".to_owned())
            })?;
        Ok(Self { client })
    }

    async fn endpoint_is_healthy(&self, endpoint: &str) -> bool {
        self.client
            .get(endpoint)
            .send()
            .await
            .is_ok_and(|response| response.status() == StatusCode::NO_CONTENT)
    }
}

#[async_trait]
impl RouteHealthProbe for HttpsRouteHealthProbe {
    async fn is_healthy(&self) -> bool {
        let (google, cloudflare) = tokio::join!(
            self.endpoint_is_healthy(GOOGLE_CONNECTIVITY_CHECK),
            self.endpoint_is_healthy(CLOUDFLARE_CONNECTIVITY_CHECK),
        );
        google || cloudflare
    }
}
