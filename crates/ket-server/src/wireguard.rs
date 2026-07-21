use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use ket_core::{SecretString, TransportCredential};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::config::WireGuardConfig;

const PRIVATE_KEY_CONTEXT: &[u8] = b"ket-wireguard-client-private-key-v1\0";
const PRESHARED_KEY_CONTEXT: &[u8] = b"ket-wireguard-preshared-key-v1\0";

#[derive(Clone, Deserialize, Serialize)]
pub struct ManagedPeer {
    pub private_key: SecretString,
    pub preshared_key: SecretString,
    pub address: String,
}

impl std::fmt::Debug for ManagedPeer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedPeer")
            .field("private_key", &"[REDACTED]")
            .field("preshared_key", &"[REDACTED]")
            .field("address", &self.address)
            .finish()
    }
}

#[derive(Deserialize, Serialize)]
pub struct ReconcilePeers {
    pub peers: Vec<ManagedPeer>,
}

#[derive(Deserialize, Serialize)]
pub struct RemovePeers {
    pub addresses: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerStatus {
    pub public_key: String,
    pub address: String,
    pub latest_handshake_epoch_seconds: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

pub(crate) fn managed_peer(
    config: &WireGuardConfig,
    session_id: &str,
    resource_slot: u32,
) -> ManagedPeer {
    ManagedPeer {
        private_key: derive_key(config, PRIVATE_KEY_CONTEXT, session_id),
        preshared_key: derive_key(config, PRESHARED_KEY_CONTEXT, session_id),
        address: config
            .client_address(resource_slot)
            .expect("validated WireGuard capacity covers every resource slot"),
    }
}

pub(crate) fn session_credential(
    config: &WireGuardConfig,
    session_id: &str,
) -> TransportCredential {
    let peer = managed_peer(config, session_id, 0);
    TransportCredential {
        auth: peer.private_key,
        secrets: BTreeMap::from([
            ("preshared_key".to_owned(), peer.preshared_key),
            (
                "server_public_key".to_owned(),
                SecretString::from(config.server_public_key.as_str()),
            ),
        ]),
    }
}

fn derive_key(config: &WireGuardConfig, context: &[u8], session_id: &str) -> SecretString {
    let mut mac = Hmac::<Sha256>::new_from_slice(config.credential_key.as_bytes())
        .expect("validated WireGuard credential key is accepted by HMAC");
    mac.update(context);
    mac.update(session_id.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_keys_are_stable_scoped_and_redacted() {
        let config = test_config();
        let first = managed_peer(&config, "AbCdEf123456", 0);
        let same = managed_peer(&config, "AbCdEf123456", 0);
        let other = managed_peer(&config, "Other1234567", 1);
        assert_eq!(first.private_key, same.private_key);
        assert_ne!(first.private_key, other.private_key);
        assert_ne!(first.private_key, first.preshared_key);
        assert_eq!(first.address, "10.66.0.2");
        assert_eq!(other.address, "10.66.0.3");
        for key in [&first.private_key, &first.preshared_key] {
            assert_eq!(STANDARD.decode(key.expose_secret()).unwrap().len(), 32);
            assert!(!format!("{first:?}").contains(key.expose_secret()));
        }
        let credential = session_credential(&config, "AbCdEf123456");
        assert_eq!(credential.auth, first.private_key);
        assert_eq!(
            credential.secrets["server_public_key"].expose_secret(),
            config.server_public_key
        );
    }

    fn test_config() -> WireGuardConfig {
        WireGuardConfig {
            transport_id: "wireguard-tls-primary".to_owned(),
            manager_url: "http://127.0.0.1:8788".to_owned(),
            manager_token: "manager-token-with-at-least-32-characters".to_owned(),
            credential_key: "credential-key-with-at-least-32-characters".to_owned(),
            server_public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            public_port: 443,
            sni: "vpn.example.test".to_owned(),
            path_prefix: "ket-wireguard-test".to_owned(),
        }
    }
}
