use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::SecretString;

pub const MAX_ACCESS_GRANT_VALIDITY_MINUTES: u32 = 365 * 24 * 60;

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
    pub valid_for_minutes: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateAccessGrantBatchRequest {
    pub label_prefix: String,
    pub count: u16,
    pub max_connections: u32,
    pub valid_for_minutes: u32,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct CreateAccessGrantResponse {
    pub id: String,
    /// Returned exactly once. Ket never stores this plaintext value.
    pub access_code: SecretString,
    pub label: String,
    pub max_connections: u32,
    pub valid_for_minutes: u32,
    pub expires_at_epoch_seconds: u64,
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
            .field("valid_for_minutes", &self.valid_for_minutes)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_expires_at_epoch_seconds: Option<u64>,
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
            .field(
                "access_expires_at_epoch_seconds",
                &self.access_expires_at_epoch_seconds,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_expires_at_epoch_seconds: Option<u64>,
    pub node: NodeStatus,
    pub traffic: SessionTraffic,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn protocol_network_and_health_wire_names_are_stable() {
        let protocols = [
            (TransportProtocol::Hysteria2, "hysteria2"),
            (TransportProtocol::Ikev2, "ikev2"),
            (TransportProtocol::OpenVpnStunnel, "open_vpn_stunnel"),
            (TransportProtocol::Shadowsocks2022, "shadowsocks2022"),
            (TransportProtocol::Stealth, "stealth"),
            (TransportProtocol::VlessXtlsReality, "vless_xtls_reality"),
            (TransportProtocol::WireGuard, "wire_guard"),
            (TransportProtocol::XorScrambled, "xor_scrambled"),
        ];
        for (value, wire_name) in protocols {
            assert_wire_round_trip(value, json!(wire_name));
        }
        for (value, wire_name) in [
            (Network::Tcp, "tcp"),
            (Network::Udp, "udp"),
            (Network::TcpAndUdp, "tcp_and_udp"),
        ] {
            assert_wire_round_trip(value, json!(wire_name));
        }
        for (value, wire_name) in [
            (HealthState::Healthy, "healthy"),
            (HealthState::Degraded, "degraded"),
            (HealthState::Saturated, "saturated"),
        ] {
            assert_wire_round_trip(value, json!(wire_name));
        }
    }

    #[test]
    fn session_manifest_round_trips_without_leaking_secrets_to_debug() {
        let manifest = test_manifest();
        let value = serde_json::to_value(&manifest).expect("manifest must serialize");
        assert_eq!(value["session_token"], "session-secret");
        assert_eq!(
            value["transports"][0]["credential"]["auth"],
            "transport-secret"
        );
        assert_eq!(
            value["transports"][0]["credential"]["secrets"]["private_key"],
            "private-secret"
        );
        assert!(value["transports"][0].get("profile").is_none());
        assert!(value["transports"][0].get("tls_server_name").is_some());

        let decoded: SessionManifest =
            serde_json::from_value(value).expect("manifest must deserialize");
        assert_eq!(decoded, manifest);

        let debug = format!("{manifest:?}");
        for secret in ["session-secret", "transport-secret", "private-secret"] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn optional_and_default_contract_fields_are_backward_compatible() {
        let profile: TransportProfile = serde_json::from_value(json!({
            "id": "hy2-primary",
            "display_name": "Hysteria 2",
            "protocol": "hysteria2",
            "endpoint": "vpn.example.test",
            "port": 443,
            "network": "udp",
            "priority": 1
        }))
        .expect("optional profile fields must default");
        assert_eq!(profile.tls_server_name, None);
        assert!(profile.options.is_empty());

        let encoded = serde_json::to_value(&profile).expect("profile must serialize");
        assert!(encoded.get("tls_server_name").is_none());
        assert_eq!(encoded["options"], json!({}));

        let transport: SessionTransport = serde_json::from_value(encoded)
            .expect("missing credentials must represent a public profile");
        assert_eq!(transport.credential, None);
    }

    #[test]
    fn grant_and_status_contracts_preserve_lifetime_and_telemetry_fields() {
        let request = CreateAccessGrantRequest {
            label: "Personal devices".to_owned(),
            max_connections: 5,
            valid_for_minutes: MAX_ACCESS_GRANT_VALIDITY_MINUTES,
        };
        assert_wire_round_trip(
            request,
            json!({
                "label": "Personal devices",
                "max_connections": 5,
                "valid_for_minutes": 525600
            }),
        );

        let status = SessionStatus {
            session_id: "AbCdEf123456".to_owned(),
            client_name: "Ket Android".to_owned(),
            expires_at_epoch_seconds: 2_000,
            access_expires_at_epoch_seconds: Some(3_000),
            node: test_node(),
            traffic: SessionTraffic {
                available: true,
                bytes_sent: 10,
                bytes_received: 20,
                online_connections: 1,
                observed_at_epoch_seconds: 1_500,
            },
        };
        let value = serde_json::to_value(&status).expect("status must serialize");
        assert_eq!(value["access_expires_at_epoch_seconds"], 3_000);
        assert_eq!(value["traffic"]["bytes_received"], 20);
        assert_wire_round_trip(status, value);
    }

    fn assert_wire_round_trip<T>(value: T, expected: Value)
    where
        T: Clone + std::fmt::Debug + PartialEq + Serialize + DeserializeOwned,
    {
        let encoded = serde_json::to_value(&value).expect("value must serialize");
        assert_eq!(encoded, expected);
        let decoded: T = serde_json::from_value(encoded).expect("value must deserialize");
        assert_eq!(decoded, value);
    }

    fn test_manifest() -> SessionManifest {
        SessionManifest {
            session_token: SecretString::from("session-secret"),
            session_expires_at_epoch_seconds: 2_000,
            access_expires_at_epoch_seconds: Some(3_000),
            node: test_node(),
            transports: vec![SessionTransport {
                profile: TransportProfile {
                    id: "reality-primary".to_owned(),
                    display_name: "VLESS + REALITY".to_owned(),
                    protocol: TransportProtocol::VlessXtlsReality,
                    endpoint: "vpn.example.test".to_owned(),
                    port: 443,
                    network: Network::Tcp,
                    priority: 1,
                    tls_server_name: Some("www.example.com".to_owned()),
                    options: BTreeMap::from([("flow".to_owned(), "xtls-rprx-vision".to_owned())]),
                },
                credential: Some(TransportCredential {
                    auth: SecretString::from("transport-secret"),
                    secrets: BTreeMap::from([(
                        "private_key".to_owned(),
                        SecretString::from("private-secret"),
                    )]),
                }),
            }],
        }
    }

    fn test_node() -> NodeStatus {
        NodeStatus {
            node_id: "sg-01".to_owned(),
            display_name: "Ket Singapore".to_owned(),
            public_url: "https://ket.example.test".to_owned(),
            location: NodeLocation {
                country_code: "SG".to_owned(),
                country_name: "Singapore".to_owned(),
                city: Some("Singapore".to_owned()),
                latitude: 1.29,
                longitude: 103.85,
            },
            health: HealthState::Healthy,
            active_sessions: 1,
            session_capacity: 10,
            capacity_percent: 10.0,
            cpu_load_percent: Some(5.0),
            memory_used_bytes: Some(1_024),
            memory_total_bytes: Some(4_096),
            uptime_seconds: Some(60),
            observed_at_epoch_seconds: 1_000,
        }
    }
}
