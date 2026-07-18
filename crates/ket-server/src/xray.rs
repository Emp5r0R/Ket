use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use ket_core::{SecretString, TransportCredential};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::config::XrayConfig;

const UUID_CONTEXT: &[u8] = b"ket-vless-reality-session-v1\0";
const PRIVATE_DESTINATIONS: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "224.0.0.0/4",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

pub async fn write_runtime_config(config: &XrayConfig) -> Result<()> {
    let path = config.runtime_config_path.clone();
    let document = render_runtime_config(config);
    tokio::task::spawn_blocking(move || write_atomic(&path, &document))
        .await
        .context("Xray configuration writer stopped unexpectedly")??;
    Ok(())
}

pub(crate) fn session_email(session_id: &str) -> String {
    format!("session-{session_id}@ket.invalid")
}

pub(crate) fn session_uuid(config: &XrayConfig, session_id: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(config.credential_key.as_bytes())
        .expect("validated Xray credential key is accepted by HMAC");
    mac.update(UUID_CONTEXT);
    mac.update(session_id.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

pub(crate) fn session_credential(config: &XrayConfig, session_id: &str) -> TransportCredential {
    let mut secrets = BTreeMap::new();
    secrets.insert(
        "reality_password".to_owned(),
        SecretString::from(config.public_key.as_str()),
    );
    secrets.insert(
        "reality_short_id".to_owned(),
        SecretString::from(config.short_id.as_str()),
    );
    TransportCredential {
        auth: session_uuid(config, session_id).into(),
        secrets,
    }
}

pub(crate) fn user_document(config: &XrayConfig, session_id: &str) -> Value {
    json!({
        "inbounds": [{
            "listen": config.listen_host,
            "port": config.listen_port,
            "tag": config.inbound_tag,
            "protocol": "vless",
            "settings": {
                "clients": [{
                    "id": session_uuid(config, session_id),
                    "email": session_email(session_id),
                    "flow": "xtls-rprx-vision"
                }],
                "decryption": "none"
            }
        }]
    })
}

fn render_runtime_config(config: &XrayConfig) -> Value {
    json!({
        "log": {
            "access": "none",
            "dnsLog": false,
            "loglevel": "warning"
        },
        "api": {
            "tag": "api",
            "services": ["HandlerService", "StatsService"]
        },
        "stats": {},
        "policy": {
            "levels": {
                "0": {
                    "statsUserUplink": true,
                    "statsUserDownlink": true
                }
            },
            "system": {
                "statsInboundUplink": true,
                "statsInboundDownlink": true
            }
        },
        "inbounds": [
            {
                "listen": config.api_listen,
                "port": config.api_port,
                "protocol": "dokodemo-door",
                "tag": "api-in",
                "settings": { "address": "127.0.0.1" }
            },
            {
                "listen": config.listen_host,
                "port": config.listen_port,
                "protocol": "vless",
                "tag": config.inbound_tag,
                "settings": {
                    "clients": [],
                    "decryption": "none"
                },
                "streamSettings": {
                    "network": "raw",
                    "security": "reality",
                    "realitySettings": {
                        "show": false,
                        "target": config.reality_target,
                        "xver": 0,
                        "serverNames": config.server_names,
                        "privateKey": config.private_key,
                        "shortIds": [config.short_id]
                    }
                },
                "sniffing": {
                    "enabled": true,
                    "destOverride": ["http", "tls", "quic"],
                    "routeOnly": true
                }
            }
        ],
        "outbounds": [
            {
                "protocol": "freedom",
                "tag": "direct",
                "settings": { "domainStrategy": "UseIP" }
            },
            {
                "protocol": "blackhole",
                "tag": "blocked"
            }
        ],
        "routing": {
            "domainStrategy": "IPIfNonMatch",
            "rules": [
                {
                    "type": "field",
                    "inboundTag": ["api-in"],
                    "outboundTag": "api"
                },
                {
                    "type": "field",
                    "ip": PRIVATE_DESTINATIONS,
                    "outboundTag": "blocked"
                },
                {
                    "type": "field",
                    "network": "tcp",
                    "port": "25,465,587",
                    "outboundTag": "blocked"
                },
                {
                    "type": "field",
                    "protocol": ["bittorrent"],
                    "outboundTag": "blocked"
                }
            ]
        }
    })
}

fn write_atomic(path: &Path, document: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temporary_path = temporary_path(path);
    let encoded = serde_json::to_vec_pretty(document)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary_path)
        .with_context(|| format!("failed to open {}", temporary_path.display()))?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::rename(&temporary_path, path)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_enables_reality_api_stats_and_abuse_guards() {
        let document = render_runtime_config(&test_config());
        assert_eq!(document["api"]["services"][0], "HandlerService");
        assert_eq!(
            document["inbounds"][1]["streamSettings"]["security"],
            "reality"
        );
        assert_eq!(
            document["inbounds"][1]["streamSettings"]["realitySettings"]["target"],
            "www.example.com:443"
        );
        let rules = document["routing"]["rules"]
            .as_array()
            .expect("routing rules must be an array");
        assert!(rules.iter().any(|rule| rule["port"] == "25,465,587"));
        assert!(rules.iter().any(|rule| rule["protocol"][0] == "bittorrent"));
    }

    #[test]
    fn credentials_are_stable_scoped_and_redacted() {
        let config = test_config();
        let first = session_credential(&config, "AbCdEf123456");
        let same = session_credential(&config, "AbCdEf123456");
        let other = session_credential(&config, "Other1234567");
        assert_eq!(first, same);
        assert_ne!(first.auth, other.auth);
        assert_eq!(first.auth.len(), 36);
        assert_eq!(&first.auth.expose_secret()[14..15], "4");
        assert!(!format!("{first:?}").contains(config.public_key.as_str()));
        assert_eq!(
            first.secrets.get("reality_short_id").expect("short ID"),
            &SecretString::from(config.short_id.as_str())
        );
    }

    #[test]
    fn dynamic_user_document_uses_vision_and_session_email() {
        let config = test_config();
        let document = user_document(&config, "AbCdEf123456");
        assert_eq!(document["inbounds"][0]["tag"], config.inbound_tag);
        assert_eq!(
            document["inbounds"][0]["settings"]["clients"][0]["flow"],
            "xtls-rprx-vision"
        );
        assert_eq!(
            document["inbounds"][0]["settings"]["clients"][0]["email"],
            "session-AbCdEf123456@ket.invalid"
        );
    }

    fn test_config() -> XrayConfig {
        XrayConfig {
            transport_id: "vless-reality-primary".to_owned(),
            runtime_config_path: "/tmp/xray.json".into(),
            binary_path: "/usr/local/bin/xray".into(),
            api_server: "xray:10085".to_owned(),
            api_listen: "0.0.0.0".to_owned(),
            api_port: 10085,
            inbound_tag: "vless-reality".to_owned(),
            listen_host: "0.0.0.0".to_owned(),
            listen_port: 8444,
            public_host: "vpn.example.test".to_owned(),
            public_port: 443,
            sni: "www.example.com".to_owned(),
            server_names: vec!["www.example.com".to_owned()],
            reality_target: "www.example.com:443".to_owned(),
            private_key: "sNiYhhj3HlKqBYe7F8XKtMP2h9lV4piS0HgXwBbELGU".to_owned(),
            public_key: "GMUeujXct7_Ig4N9J5asVItA8mXOMXBXGzcdMowh5Ag".to_owned(),
            short_id: "0123456789abcdef".to_owned(),
            credential_key: "credential-key-with-at-least-32-characters".to_owned(),
            fingerprint: "chrome".to_owned(),
        }
    }
}
