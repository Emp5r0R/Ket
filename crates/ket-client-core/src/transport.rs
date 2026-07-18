use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use ket_core::SessionTransport;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::ClientError;

#[derive(Clone, Debug)]
pub struct ProbeReport {
    pub resolved_addresses: Vec<SocketAddr>,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "message")]
pub enum TunnelStatus {
    Connected,
    Stopped,
    Failed(String),
}

#[async_trait]
pub trait ActiveTunnel: Send + Sync {
    fn transport_id(&self) -> &str;
    fn status(&self) -> watch::Receiver<TunnelStatus>;
    async fn stop(&self) -> Result<(), ClientError>;
}

pub struct StartedTunnel {
    pub tunnel: Arc<dyn ActiveTunnel>,
    pub handshake_latency: Duration,
}

#[async_trait]
pub trait TransportAdapter: Send + Sync {
    fn supports(&self, transport: &SessionTransport) -> bool;
    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError>;
    async fn connect(
        &self,
        transport: &SessionTransport,
        probe: &ProbeReport,
    ) -> Result<StartedTunnel, ClientError>;
}
