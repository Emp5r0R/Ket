use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use ket_core::{SecretString, TransportCredential};
use sha2::Sha256;

use crate::config::ShadowsocksConfig;

const KEY_CONTEXT: &[u8] = b"ket-shadowsocks-2022-session-key-v1\0";

pub(crate) fn session_key(config: &ShadowsocksConfig, session_id: &str) -> SecretString {
    let mut mac = Hmac::<Sha256>::new_from_slice(config.credential_key.as_bytes())
        .expect("validated Shadowsocks credential key is accepted by HMAC");
    mac.update(KEY_CONTEXT);
    mac.update(session_id.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes()).into()
}

pub(crate) fn session_credential(
    config: &ShadowsocksConfig,
    session_id: &str,
) -> TransportCredential {
    TransportCredential {
        auth: session_key(config, session_id),
        secrets: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::*;

    #[test]
    fn session_keys_are_scoped_stable_and_redacted() {
        let config = test_config();
        let first = session_credential(&config, "Session00001");
        let same = session_credential(&config, "Session00001");
        let other = session_credential(&config, "Session00002");

        assert_eq!(first, same);
        assert_ne!(first, other);
        assert_eq!(
            STANDARD
                .decode(first.auth.expose_secret())
                .expect("key must use standard base64")
                .len(),
            32
        );
        assert!(first.secrets.is_empty());
        assert!(!format!("{first:?}").contains(first.auth.expose_secret()));
    }

    fn test_config() -> ShadowsocksConfig {
        ShadowsocksConfig {
            transport_id: "shadowsocks-2022-primary".to_owned(),
            manager_address: "127.0.0.1:6100".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            port_start: 20_000,
            port_end: 20_999,
            credential_key: "independent-test-key-at-least-32-characters".to_owned(),
        }
    }
}
