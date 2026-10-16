//! The admin contract for managing access grants (invites) after they are
//! created: which protocols an invite may use, pausing, editing, and what is
//! connected through it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{AccessGrantSummary, SessionTraffic, TransportProtocol};

/// Which of the server's protocols an access grant may use. `All` also
/// covers protocols the server enables later.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolAccess {
    #[default]
    All,
    Only(BTreeSet<TransportProtocol>),
}

impl ProtocolAccess {
    pub fn allows(&self, protocol: &TransportProtocol) -> bool {
        match self {
            Self::All => true,
            Self::Only(protocols) => protocols.contains(protocol),
        }
    }

    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// An empty `Only` set would lock every device out, so it is never valid.
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Only(protocols) if protocols.is_empty())
    }
}

impl TransportProtocol {
    /// The short name the server's `ket` command accepts and prints.
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Hysteria2 => "hysteria",
            Self::Ikev2 => "ikev2",
            Self::OpenVpnStunnel => "openvpn",
            Self::Shadowsocks2022 => "shadowsocks",
            Self::Stealth => "stealth",
            Self::VlessXtlsReality => "reality",
            Self::WireGuard => "wireguard",
            Self::XorScrambled => "hysteria-xor",
        }
    }
}

/// A protocol this server offers, as the admin API lists it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfferedProtocol {
    pub protocol: TransportProtocol,
    pub name: String,
    pub display_name: String,
}

/// Changes to an existing access grant. Absent fields stay as they are.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAccessGrantRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_epoch_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocols: Option<ProtocolAccess>,
}

impl UpdateAccessGrantRequest {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UpdateAccessGrantResponse {
    pub grant: AccessGrantSummary,
    /// Devices whose sessions the change ended. They connect again with the
    /// new settings, or are refused if the grant no longer lets them in.
    pub disconnected_devices: u32,
}

/// One device connected through an access grant.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConnectedDevice {
    pub session_id: String,
    pub client_name: String,
    pub connected_at_epoch_seconds: u64,
    pub session_expires_at_epoch_seconds: u64,
    pub traffic: SessionTraffic,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AccessGrantDetails {
    #[serde(flatten)]
    pub grant: AccessGrantSummary,
    pub devices: Vec<ConnectedDevice>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn protocol_access_has_stable_wire_names_and_never_allows_an_empty_set() {
        assert_eq!(
            serde_json::to_value(ProtocolAccess::All).unwrap(),
            json!("all")
        );
        let only = ProtocolAccess::Only(BTreeSet::from([
            TransportProtocol::Stealth,
            TransportProtocol::VlessXtlsReality,
        ]));
        assert_eq!(
            serde_json::to_value(&only).unwrap(),
            json!({"only": ["stealth", "vless_xtls_reality"]})
        );
        assert_eq!(
            serde_json::from_value::<ProtocolAccess>(json!({"only": ["stealth"]})).unwrap(),
            ProtocolAccess::Only(BTreeSet::from([TransportProtocol::Stealth]))
        );
        assert!(only.allows(&TransportProtocol::Stealth));
        assert!(!only.allows(&TransportProtocol::WireGuard));
        assert!(ProtocolAccess::All.allows(&TransportProtocol::WireGuard));
        assert!(only.is_valid() && ProtocolAccess::All.is_valid());
        assert!(!ProtocolAccess::Only(BTreeSet::new()).is_valid());
    }

    #[test]
    fn short_names_are_distinct() {
        let protocols = [
            TransportProtocol::Hysteria2,
            TransportProtocol::Ikev2,
            TransportProtocol::OpenVpnStunnel,
            TransportProtocol::Shadowsocks2022,
            TransportProtocol::Stealth,
            TransportProtocol::VlessXtlsReality,
            TransportProtocol::WireGuard,
            TransportProtocol::XorScrambled,
        ];
        let names = protocols
            .iter()
            .map(TransportProtocol::short_name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), protocols.len());
    }

    #[test]
    fn updates_reject_unknown_fields_and_know_when_they_change_nothing() {
        assert!(serde_json::from_value::<UpdateAccessGrantRequest>(json!({"expiry": 5})).is_err());
        let empty: UpdateAccessGrantRequest = serde_json::from_value(json!({})).unwrap();
        assert!(empty.is_empty());
        let pause: UpdateAccessGrantRequest =
            serde_json::from_value(json!({"paused": true})).unwrap();
        assert!(!pause.is_empty());
        assert_eq!(
            serde_json::to_value(&pause).unwrap(),
            json!({"paused": true})
        );
    }
}
