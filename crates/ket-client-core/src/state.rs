use ket_core::{Network, NodeStatus, SessionTraffic, TransportProfile, TransportProtocol};
use serde::{Deserialize, Serialize};

use crate::ClientIssue;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPhase {
    #[default]
    Disconnected,
    Enrolling,
    Enrolled,
    Probing,
    Connecting,
    Connected,
    Reconnecting,
    Disconnecting,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TransportSummary {
    pub id: String,
    pub display_name: String,
    pub protocol: TransportProtocol,
    pub network: Network,
}

impl From<&TransportProfile> for TransportSummary {
    fn from(profile: &TransportProfile) -> Self {
        Self {
            id: profile.id.clone(),
            display_name: profile.display_name.clone(),
            protocol: profile.protocol.clone(),
            network: profile.network.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientSnapshot {
    pub phase: ClientPhase,
    pub node: Option<NodeStatus>,
    pub active_transport: Option<TransportSummary>,
    pub traffic: Option<SessionTraffic>,
    pub handshake_latency_ms: Option<u64>,
    pub session_expires_at_epoch_seconds: Option<u64>,
    pub connected_at_epoch_seconds: Option<u64>,
    pub reconnect_attempt: u32,
    pub issue: Option<ClientIssue>,
    pub updated_at_epoch_seconds: u64,
}
