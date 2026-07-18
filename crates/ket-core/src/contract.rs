use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::SecretString;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NodeLocation {
    pub country_code: String,
    pub country_name: String,
    pub city: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportProtocol {
    Hysteria2,
    Ikev2,
    OpenVpnStunnel,
    Shadowsocks2022,
    Stealth,
    VlessXtlsReality,
    WireGuard,
    XorScrambled,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Tcp,
    Udp,
    TcpAndUdp,
}

/// A transport-neutral profile. Protocol adapters own the keys in `options`.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct TransportProfile {
    pub id: String,
    pub display_name: String,
    pub protocol: TransportProtocol,
    pub endpoint: String,
    pub port: u16,
    pub network: Network,
    pub priority: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_server_name: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

impl std::fmt::Debug for TransportProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportProfile")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .field("port", &self.port)
            .field("network", &self.network)
            .field("priority", &self.priority)
            .field("tls_server_name", &self.tls_server_name)
            .field("option_keys", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    Degraded,
    Saturated,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NodeStatus {
    pub node_id: String,
    pub display_name: String,
    pub public_url: String,
    pub location: NodeLocation,
    pub health: HealthState,
    pub active_sessions: u32,
    pub session_capacity: u32,
    pub capacity_percent: f32,
    pub cpu_load_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub observed_at_epoch_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateAccessGrantRequest {
    pub label: String,
    pub max_connections: u32,
    pub expires_at_epoch_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateAccessGrantBatchRequest {
    pub label_prefix: String,
    pub count: u16,
    pub max_connections: u32,
    pub expires_at_epoch_seconds: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct CreateAccessGrantResponse {
    pub id: String,
    /// Returned exactly once. Ket never stores this plaintext value.
    pub access_code: SecretString,
    pub label: String,
    pub max_connections: u32,
    pub expires_at_epoch_seconds: Option<u64>,
    pub created_at_epoch_seconds: u64,
}

impl std::fmt::Debug for CreateAccessGrantResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateAccessGrantResponse")
            .field("id", &self.id)
            .field("access_code", &self.access_code)
            .field("label", &self.label)
            .field("max_connections", &self.max_connections)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .field("created_at_epoch_seconds", &self.created_at_epoch_seconds)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AccessGrantSummary {
    pub id: String,
    pub label: String,
    pub max_connections: u32,
    pub active_connections: u32,
    pub expires_at_epoch_seconds: Option<u64>,
    pub created_at_epoch_seconds: u64,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct CreateSessionRequest {
    pub access_code: SecretString,
    pub client_name: String,
}

impl std::fmt::Debug for CreateSessionRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateSessionRequest")
            .field("access_code", &self.access_code)
            .field("client_name", &self.client_name)
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct TransportCredential {
    pub auth: SecretString,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretString>,
}

impl std::fmt::Debug for TransportCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportCredential")
            .field("auth", &self.auth)
            .field("secret_keys", &self.secrets.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionTransport {
    #[serde(flatten)]
    pub profile: TransportProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<TransportCredential>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct SessionManifest {
    pub session_token: SecretString,
    pub session_expires_at_epoch_seconds: u64,
    pub node: NodeStatus,
    pub transports: Vec<SessionTransport>,
}

impl std::fmt::Debug for SessionManifest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionManifest")
            .field("session_token", &self.session_token)
            .field(
                "session_expires_at_epoch_seconds",
                &self.session_expires_at_epoch_seconds,
            )
            .field("node", &self.node)
            .field("transports", &self.transports)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionTraffic {
    pub available: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub online_connections: u32,
    pub observed_at_epoch_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionStatus {
    pub session_id: String,
    pub client_name: String,
    pub expires_at_epoch_seconds: u64,
    pub node: NodeStatus,
    pub traffic: SessionTraffic,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}
