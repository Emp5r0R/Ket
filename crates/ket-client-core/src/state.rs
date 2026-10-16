use ket_core::{
    ClientUpdate, Network, NodeStatus, SessionTraffic, TransportProfile, TransportProtocol,
    UpdateAdvice,
};
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

/// A transport the latest connection attempt tried and left, and why, so an
/// app can say why it is not using the protocol it tried first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TransportFallback {
    pub transport: TransportSummary,
    pub issue: ClientIssue,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientSnapshot {
    pub phase: ClientPhase,
    pub node: Option<NodeStatus>,
    pub available_transports: Vec<TransportSummary>,
    pub preferred_protocol: Option<TransportProtocol>,
    pub active_transport: Option<TransportSummary>,
    pub traffic: Option<SessionTraffic>,
    pub handshake_latency_ms: Option<u64>,
    pub session_expires_at_epoch_seconds: Option<u64>,
    pub access_expires_at_epoch_seconds: Option<u64>,
    pub connected_at_epoch_seconds: Option<u64>,
    pub reconnect_attempt: u32,
    pub issue: Option<ClientIssue>,
    /// The transports the latest connection attempt gave up on, in order.
    #[serde(default)]
    pub fallbacks: Vec<TransportFallback>,
    /// The server's Ket version, from its last session response.
    #[serde(default)]
    pub server_version: Option<String>,
    /// What the server asks of this app: nothing, an optional update, or a
    /// required one.
    #[serde(default)]
    pub client_update: Option<ClientUpdate>,
    pub updated_at_epoch_seconds: u64,
}

impl ClientSnapshot {
    /// Records a session response's update advice. Servers from before the
    /// update lifecycle send none, which leaves the last advice in place.
    pub(crate) fn apply_advice(&mut self, advice: Option<UpdateAdvice>) {
        if let Some(advice) = advice {
            self.server_version = Some(advice.server_version);
            self.client_update = Some(advice.client);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ket_core::TransportProfile;

    use super::*;

    #[test]
    fn phase_wire_names_and_disconnected_default_are_stable() {
        let phases = [
            (ClientPhase::Disconnected, "disconnected"),
            (ClientPhase::Enrolling, "enrolling"),
            (ClientPhase::Enrolled, "enrolled"),
            (ClientPhase::Probing, "probing"),
            (ClientPhase::Connecting, "connecting"),
            (ClientPhase::Connected, "connected"),
            (ClientPhase::Reconnecting, "reconnecting"),
            (ClientPhase::Disconnecting, "disconnecting"),
            (ClientPhase::Error, "error"),
        ];
        for (phase, wire_name) in phases {
            assert_eq!(
                serde_json::to_string(&phase).unwrap(),
                format!("\"{wire_name}\"")
            );
            assert_eq!(
                serde_json::from_str::<ClientPhase>(&format!("\"{wire_name}\"")).unwrap(),
                phase
            );
        }

        let snapshot = ClientSnapshot::default();
        assert_eq!(snapshot.phase, ClientPhase::Disconnected);
        assert!(snapshot.node.is_none());
        assert!(snapshot.available_transports.is_empty());
        assert!(snapshot.issue.is_none());
    }

    #[test]
    fn transport_summary_exposes_identity_but_not_adapter_options() {
        let profile = TransportProfile {
            id: "stealth-primary".to_owned(),
            display_name: "HTTPS Stealth".to_owned(),
            protocol: TransportProtocol::Stealth,
            endpoint: "secret-origin.example.test".to_owned(),
            port: 443,
            network: Network::Tcp,
            priority: 1,
            tls_server_name: Some("public.example.test".to_owned()),
            options: BTreeMap::from([("path".to_owned(), "/private-path".to_owned())]),
        };
        let summary = TransportSummary::from(&profile);
        assert_eq!(summary.id, "stealth-primary");
        assert_eq!(summary.protocol, TransportProtocol::Stealth);
        let encoded = serde_json::to_value(summary).unwrap();
        assert_eq!(encoded["network"], "tcp");
        assert!(encoded.get("endpoint").is_none());
        assert!(encoded.get("options").is_none());
        assert!(!encoded.to_string().contains("private-path"));
    }
}
