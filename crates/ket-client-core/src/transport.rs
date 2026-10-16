use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use ket_core::SessionTransport;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{ClientError, ConnectOptions};

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
        options: &ConnectOptions,
    ) -> Result<StartedTunnel, ClientError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_status_wire_contract_round_trips_all_states() {
        for (status, expected) in [
            (TunnelStatus::Connected, r#"{"state":"connected"}"#),
            (TunnelStatus::Stopped, r#"{"state":"stopped"}"#),
            (
                TunnelStatus::Failed("route lost".to_owned()),
                r#"{"state":"failed","message":"route lost"}"#,
            ),
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            assert_eq!(encoded, expected);
            assert_eq!(
                serde_json::from_str::<TunnelStatus>(&encoded).unwrap(),
                status
            );
        }
    }
}
