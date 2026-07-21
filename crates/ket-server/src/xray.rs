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

use crate::config::{XrayConfig, XrayRealityConfig, XrayXhttpConfig};

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

pub(crate) fn session_credential(
    config: &XrayConfig,
    transport_id: &str,
    session_id: &str,
) -> Option<TransportCredential> {
    let mut secrets = BTreeMap::new();
    if let Some(reality) = config
        .reality
        .as_ref()
        .filter(|reality| reality.transport_id == transport_id)
    {
        secrets.insert(
            "reality_password".to_owned(),
            SecretString::from(reality.public_key.as_str()),
        );
        secrets.insert(
            "reality_short_id".to_owned(),
            SecretString::from(reality.short_id.as_str()),
        );
    } else if config
        .xhttp
        .as_ref()
        .is_none_or(|xhttp| xhttp.transport_id != transport_id)
    {
        return None;
    }
    Some(TransportCredential {
        auth: session_uuid(config, session_id).into(),
        secrets,
    })
}

pub(crate) fn inbound_tags(config: &XrayConfig) -> Vec<&str> {
    config
        .reality
        .iter()
        .map(|reality| reality.inbound_tag.as_str())
        .chain(config.xhttp.iter().map(|xhttp| xhttp.inbound_tag.as_str()))
        .collect()
}

pub(crate) fn user_documents(config: &XrayConfig, session_id: &str) -> Vec<Value> {
    let uuid = session_uuid(config, session_id);
    let email = session_email(session_id);
    let reality = config.reality.iter().map(|reality| {
        user_document(
            &reality.inbound_tag,
            &reality.listen_host,
            reality.listen_port,
            &uuid,
            &email,
            Some("xtls-rprx-vision"),
        )
    });
    let xhttp = config.xhttp.iter().map(|xhttp| {
        user_document(
            &xhttp.inbound_tag,
            &xhttp.listen_host,
            xhttp.listen_port,
            &uuid,
            &email,
            None,
        )
    });
    reality.chain(xhttp).collect()
}

fn user_document(
    tag: &str,
    listen_host: &str,
    listen_port: u16,
    uuid: &str,
    email: &str,
    flow: Option<&str>,
) -> Value {
    let mut client = json!({
        "id": uuid,
        "email": email
    });
    if let Some(flow) = flow {
        client["flow"] = flow.into();
    }
    json!({
        "inbounds": [{
            "listen": listen_host,
            "port": listen_port,
            "tag": tag,
            "protocol": "vless",
            "settings": {
                "clients": [client],
                "decryption": "none"
            }
        }]
    })
}

fn render_runtime_config(config: &XrayConfig) -> Value {
    let mut inbounds = vec![json!({
        "listen": config.api_listen,
        "port": config.api_port,
        "protocol": "dokodemo-door",
        "tag": "api-in",
        "settings": { "address": "127.0.0.1" }
    })];
    if let Some(reality) = &config.reality {
        inbounds.push(reality_inbound(reality));
    }
    if let Some(xhttp) = &config.xhttp {
        inbounds.push(xhttp_inbound(xhttp));
    }
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
        "inbounds": inbounds,
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

fn reality_inbound(config: &XrayRealityConfig) -> Value {
    json!({
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
        "sniffing": sniffing()
    })
}

fn xhttp_inbound(config: &XrayXhttpConfig) -> Value {
    json!({
        "listen": config.listen_host,
        "port": config.listen_port,
        "protocol": "vless",
        "tag": config.inbound_tag,
        "settings": {
            "clients": [],
            "decryption": "none"
        },
        "streamSettings": {
            "network": "xhttp",
            "security": "none",
            "xhttpSettings": {
                "path": config.path,
                "mode": "packet-up"
            }
        },
        "sniffing": sniffing()
    })
}

fn sniffing() -> Value {
    json!({
        "enabled": true,
        "destOverride": ["http", "tls", "quic"],
        "routeOnly": true
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
    use std::{
        net::TcpListener,
        process::{Child, Command, Stdio},
        thread::sleep,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

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
        assert_eq!(
            document["inbounds"][2]["streamSettings"]["network"],
            "xhttp"
        );
        assert_eq!(
            document["inbounds"][2]["streamSettings"]["security"],
            "none"
        );
        assert_eq!(
            document["inbounds"][2]["streamSettings"]["xhttpSettings"]["mode"],
            "packet-up"
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
        let reality_id = &config
            .reality
            .as_ref()
            .expect("Reality config")
            .transport_id;
        let xhttp_id = &config.xhttp.as_ref().expect("XHTTP config").transport_id;
        let first =
            session_credential(&config, reality_id, "AbCdEf123456").expect("Reality credential");
        let same =
            session_credential(&config, reality_id, "AbCdEf123456").expect("Reality credential");
        let other =
            session_credential(&config, reality_id, "Other1234567").expect("Reality credential");
        assert_eq!(first, same);
        assert_ne!(first.auth, other.auth);
        assert_eq!(first.auth.len(), 36);
        assert_eq!(&first.auth.expose_secret()[14..15], "4");
        let reality = config.reality.as_ref().expect("Reality config");
        assert!(!format!("{first:?}").contains(reality.public_key.as_str()));
        assert_eq!(
            first.secrets.get("reality_short_id").expect("short ID"),
            &SecretString::from(reality.short_id.as_str())
        );
        let xhttp =
            session_credential(&config, xhttp_id, "AbCdEf123456").expect("XHTTP credential");
        assert_eq!(xhttp.auth, first.auth);
        assert!(xhttp.secrets.is_empty());
        assert!(session_credential(&config, "unknown", "AbCdEf123456").is_none());
    }

    #[test]
    fn dynamic_user_documents_cover_reality_and_xhttp() {
        let config = test_config();
        let documents = user_documents(&config, "AbCdEf123456");
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0]["inbounds"][0]["tag"], "vless-reality");
        assert_eq!(documents[0]["inbounds"][0]["port"], 8444);
        assert_eq!(
            documents[0]["inbounds"][0]["settings"]["clients"][0]["flow"],
            "xtls-rprx-vision"
        );
        assert_eq!(
            documents[1]["inbounds"][0]["settings"]["clients"][0]["email"],
            "session-AbCdEf123456@ket.invalid"
        );
        assert_eq!(documents[1]["inbounds"][0]["port"], 8445);
        assert!(
            documents[1]["inbounds"][0]["settings"]["clients"][0]
                .get("flow")
                .is_none()
        );
        assert_eq!(inbound_tags(&config), ["vless-reality", "vless-xhttp"]);
    }

    #[test]
    fn pinned_xray_accepts_multi_inbound_runtime_config_when_supplied() {
        let Some(binary) = std::env::var_os("KET_TEST_XRAY_BINARY") else {
            return;
        };
        let binary = PathBuf::from(binary);
        let binary = if binary.is_absolute() {
            binary
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(binary)
        };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ket-xray-server-{nonce}.json"));
        fs::write(
            &path,
            serde_json::to_vec_pretty(&render_runtime_config(&test_config()))
                .expect("runtime config should encode"),
        )
        .expect("runtime config should be written");
        let status = Command::new(binary)
            .args(["run", "-test", "-c"])
            .arg(&path)
            .status()
            .expect("run Xray config validation");
        let _ = fs::remove_file(path);
        assert!(status.success());
    }

    #[test]
    fn pinned_xray_provisions_and_removes_users_from_every_inbound_when_supplied() {
        let Some(binary) = test_binary() else {
            return;
        };
        let mut config = test_config();
        config.api_listen = "127.0.0.1".to_owned();
        config.api_port = reserve_port();
        config.api_server = format!("127.0.0.1:{}", config.api_port);
        let reality = config.reality.as_mut().expect("Reality config");
        reality.listen_host = "127.0.0.1".to_owned();
        reality.listen_port = reserve_port();
        let xhttp = config.xhttp.as_mut().expect("XHTTP config");
        xhttp.listen_host = "127.0.0.1".to_owned();
        xhttp.listen_port = reserve_port();

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let runtime_path = std::env::temp_dir().join(format!("ket-xray-live-{nonce}.json"));
        fs::write(
            &runtime_path,
            serde_json::to_vec_pretty(&render_runtime_config(&config))
                .expect("runtime config should encode"),
        )
        .expect("runtime config should be written");
        let child = Command::new(&binary)
            .args(["run", "-c"])
            .arg(&runtime_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start Xray runtime");
        let _runtime = ChildGuard(child);
        let tags = inbound_tags(&config);
        wait_for_api(&binary, &config.api_server, tags[0]);

        for (index, document) in user_documents(&config, "AbCdEf123456")
            .into_iter()
            .enumerate()
        {
            let path = std::env::temp_dir().join(format!("ket-xray-user-{nonce}-{index}.json"));
            fs::write(
                &path,
                serde_json::to_vec_pretty(&document).expect("user document should encode"),
            )
            .expect("user document should be written");
            let _output = xray_api(&binary, "adu", &config.api_server, [path.as_os_str()]);
            let _ = fs::remove_file(path);
        }

        for tag in &tags {
            let output = xray_api(
                &binary,
                "inbounduser",
                &config.api_server,
                [format!("-tag={tag}")],
            );
            assert!(
                output.contains("session-AbCdEf123456@ket.invalid"),
                "provisioned user is missing: {output}"
            );
            let _output = xray_api(
                &binary,
                "rmu",
                &config.api_server,
                [
                    format!("-tag={tag}"),
                    "session-AbCdEf123456@ket.invalid".to_owned(),
                ],
            );
            let output = xray_api(
                &binary,
                "inbounduser",
                &config.api_server,
                [format!("-tag={tag}")],
            );
            assert!(
                !output.contains("session-AbCdEf123456@ket.invalid"),
                "removed user is still present: {output}"
            );
        }
        let _ = fs::remove_file(runtime_path);
    }

    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn test_binary() -> Option<PathBuf> {
        let binary = PathBuf::from(std::env::var_os("KET_TEST_XRAY_BINARY")?);
        Some(if binary.is_absolute() {
            binary
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(binary)
        })
    }

    fn reserve_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve TCP port")
            .local_addr()
            .expect("reserved local address")
            .port()
    }

    fn wait_for_api(binary: &Path, server: &str, tag: &str) {
        for _ in 0..50 {
            let output = Command::new(binary)
                .args([
                    "api",
                    "inboundusercount",
                    &format!("--server={server}"),
                    "--timeout=1",
                    &format!("-tag={tag}"),
                ])
                .stdin(Stdio::null())
                .output()
                .expect("run Xray readiness command");
            if output.status.success() {
                return;
            }
            sleep(Duration::from_millis(100));
        }
        panic!("Xray API did not become ready");
    }

    fn xray_api<I, S>(binary: &Path, operation: &str, server: &str, arguments: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = Command::new(binary)
            .args([
                "api",
                operation,
                &format!("--server={server}"),
                "--timeout=3",
            ])
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .expect("run Xray API command");
        assert!(
            output.status.success(),
            "Xray API failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("Xray API output should be UTF-8")
    }

    fn test_config() -> XrayConfig {
        XrayConfig {
            runtime_config_path: "/tmp/xray.json".into(),
            binary_path: "/usr/local/bin/xray".into(),
            api_server: "xray:10085".to_owned(),
            api_listen: "0.0.0.0".to_owned(),
            api_port: 10085,
            credential_key: "credential-key-with-at-least-32-characters".to_owned(),
            reality: Some(XrayRealityConfig {
                transport_id: "vless-reality-primary".to_owned(),
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
                fingerprint: "chrome".to_owned(),
            }),
            xhttp: Some(XrayXhttpConfig {
                transport_id: "https-stealth-primary".to_owned(),
                inbound_tag: "vless-xhttp".to_owned(),
                listen_host: "0.0.0.0".to_owned(),
                listen_port: 8445,
                public_host: "stealth.example.test".to_owned(),
                public_port: 443,
                sni: "stealth.example.test".to_owned(),
                path: "/a1b2c3d4e5f6g7h8".to_owned(),
                fingerprint: "chrome".to_owned(),
            }),
        }
    }
}
