use std::{
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use ket_core::{Network, SessionTransport, TransportProtocol};
use serde_json::json;
use tokio::{
    net::{TcpStream, lookup_host},
    process::{Child, Command},
    time::{sleep, timeout},
};

use crate::{
    ClientError, ProbeReport, StartedTunnel, TransportAdapter,
    full_route::{
        FullRouteBridge, reserve_proxy_port, stop_bridge, stop_child, supervise, wait_until_stable,
    },
    runtime::EphemeralConfig,
};

const REALITY_OPTIONS: &[&str] = &["encryption", "fingerprint", "flow", "transport"];
const REALITY_SECRETS: &[&str] = &["reality_password", "reality_short_id"];
const XHTTP_OPTIONS: &[&str] = &[
    "encryption",
    "fingerprint",
    "mode",
    "path",
    "security",
    "transport",
];
const FINGERPRINTS: &[&str] = &[
    "chrome", "firefox", "safari", "ios", "android", "edge", "random",
];

#[derive(Clone, Debug)]
pub struct XrayAdapter {
    xray_binary_path: PathBuf,
    bridge: FullRouteBridge,
    runtime_dir: PathBuf,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl XrayAdapter {
    pub fn new(
        xray_binary_path: impl Into<PathBuf>,
        bridge_binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
        dns_state_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            xray_binary_path: xray_binary_path.into(),
            bridge: FullRouteBridge::new(bridge_binary_path, dns_state_path),
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(20),
            stop_timeout: Duration::from_secs(8),
        }
    }
}

#[async_trait]
impl TransportAdapter for XrayAdapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        matches!(
            transport.profile.protocol,
            TransportProtocol::VlessXtlsReality | TransportProtocol::Stealth
        )
    }

    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
        validate_transport(transport)?;
        let started = Instant::now();
        check_binary(
            &self.xray_binary_path,
            &["version"],
            &transport.profile.id,
            "Xray",
        )
        .await?;
        self.bridge.check(&transport.profile.id).await?;
        let resolved = timeout(
            Duration::from_secs(5),
            lookup_host((transport.profile.endpoint.as_str(), transport.profile.port)),
        )
        .await
        .map_err(|_| ClientError::transport(&transport.profile.id, "DNS timed out", true))?
        .map_err(|_| {
            ClientError::transport(&transport.profile.id, "server DNS lookup failed", true)
        })?;
        let mut addresses: Vec<_> = resolved.collect();
        addresses.sort_by_key(|address| (!address.is_ipv4(), *address));
        addresses.dedup();
        if addresses.is_empty() {
            return Err(ClientError::transport(
                &transport.profile.id,
                "server DNS returned no addresses",
                true,
            ));
        }
        Ok(ProbeReport {
            resolved_addresses: addresses,
            elapsed: started.elapsed(),
        })
    }

    async fn connect(
        &self,
        transport: &SessionTransport,
        probe: &ProbeReport,
    ) -> Result<StartedTunnel, ClientError> {
        validate_transport(transport)?;
        let server = probe.resolved_addresses.first().ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                "the server address was not resolved",
                true,
            )
        })?;
        let started = Instant::now();
        let socks_port = reserve_proxy_port(&transport.profile.id).await?;
        let document = render_client_config(transport, server.ip(), socks_port)?;
        let config = EphemeralConfig::create(&self.runtime_dir, "xray", document)
            .await
            .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;

        let validation = Command::new(&self.xray_binary_path)
            .args(["run", "-test", "-c"])
            .arg(config.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status()
            .await
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to validate the Xray configuration",
                    false,
                )
            })?;
        if !validation.success() {
            return Err(ClientError::transport(
                &transport.profile.id,
                "Xray rejected the transport configuration",
                false,
            ));
        }

        let mut xray = Command::new(&self.xray_binary_path)
            .args(["run", "-c"])
            .arg(config.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to launch the Xray engine",
                    false,
                )
            })?;
        if let Err(error) = wait_for_socks(
            &mut xray,
            socks_port,
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut xray).await;
            return Err(error);
        }
        let sni = transport
            .profile
            .tls_server_name
            .as_deref()
            .expect("validated TLS server name");
        if let Err(error) =
            verify_tunnel_path(socks_port, sni, self.startup_timeout, &transport.profile.id).await
        {
            stop_child(&mut xray).await;
            return Err(error);
        }

        let mut bridge =
            self.bridge
                .start(socks_port, &probe.resolved_addresses, &transport.profile.id)?;
        if let Err(error) = wait_until_stable(
            &mut xray,
            "Xray",
            &mut bridge,
            Duration::from_millis(1200),
            &transport.profile.id,
        )
        .await
        {
            stop_bridge(&mut bridge).await;
            stop_child(&mut xray).await;
            return Err(error);
        }
        drop(config);

        Ok(StartedTunnel {
            tunnel: supervise(
                transport.profile.id.clone(),
                "Xray",
                xray,
                bridge,
                self.stop_timeout,
            ),
            handshake_latency: started.elapsed(),
        })
    }
}

pub(crate) async fn check_binary(
    path: &PathBuf,
    arguments: &[&str],
    transport_id: &str,
    name: &str,
) -> Result<(), ClientError> {
    let status = timeout(
        Duration::from_secs(5),
        Command::new(path)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| ClientError::transport(transport_id, format!("{name} check timed out"), true))?
    .map_err(|_| {
        ClientError::transport(
            transport_id,
            format!("{name} executable is unavailable"),
            false,
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(ClientError::transport(
            transport_id,
            format!("{name} executable failed its version check"),
            false,
        ))
    }
}

fn validate_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    if transport.profile.network != Network::Tcp
        || !matches!(
            transport.profile.protocol,
            TransportProtocol::VlessXtlsReality | TransportProtocol::Stealth
        )
    {
        return Err(ClientError::transport(
            id,
            "profile is not a supported Xray TCP transport",
            false,
        ));
    }
    if transport.profile.endpoint.trim().is_empty()
        || transport.profile.endpoint.contains("://")
        || transport.profile.endpoint.contains('/')
        || transport.profile.port == 0
    {
        return Err(ClientError::transport(
            id,
            "server endpoint is invalid",
            false,
        ));
    }
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .filter(|name| valid_hostname(name))
        .ok_or_else(|| ClientError::transport(id, "TLS server name is invalid", false))?;
    if sni.parse::<IpAddr>().is_ok() {
        return Err(ClientError::transport(
            id,
            "TLS server name must be a hostname",
            false,
        ));
    }
    let credential = transport
        .credential
        .as_ref()
        .ok_or_else(|| ClientError::transport(id, "transport credential is missing", false))?;
    if !valid_uuid(credential.auth.expose_secret()) {
        return Err(ClientError::transport(
            id,
            "VLESS user ID is invalid",
            false,
        ));
    }
    match transport.profile.protocol {
        TransportProtocol::VlessXtlsReality => validate_reality_transport(transport),
        TransportProtocol::Stealth => validate_xhttp_transport(transport),
        _ => unreachable!("protocol checked above"),
    }
}

fn validate_reality_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    reject_unknown_options(transport, REALITY_OPTIONS, "VLESS + REALITY")?;
    require_options(
        transport,
        &[
            ("encryption", "none"),
            ("flow", "xtls-rprx-vision"),
            ("transport", "raw"),
        ],
        "VLESS + REALITY",
    )?;
    validate_fingerprint(transport, "Reality")?;
    let credential = transport.credential.as_ref().expect("validated credential");
    if let Some(secret) = credential
        .secrets
        .keys()
        .find(|secret| !REALITY_SECRETS.contains(&secret.as_str()))
    {
        return Err(ClientError::transport(
            id,
            format!("unsupported VLESS + REALITY credential {secret}"),
            false,
        ));
    }
    let password = required_secret(transport, "reality_password")?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ClientError::transport(
            id,
            "Reality password is invalid",
            false,
        ));
    }
    let short_id = required_secret(transport, "reality_short_id")?;
    if short_id.len() != 16 || !short_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ClientError::transport(
            id,
            "Reality short ID is invalid",
            false,
        ));
    }
    Ok(())
}

fn validate_xhttp_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    reject_unknown_options(transport, XHTTP_OPTIONS, "HTTPS Stealth")?;
    require_options(
        transport,
        &[
            ("encryption", "none"),
            ("mode", "packet-up"),
            ("security", "tls"),
            ("transport", "xhttp"),
        ],
        "HTTPS Stealth",
    )?;
    validate_fingerprint(transport, "HTTPS Stealth")?;
    let path = transport
        .profile
        .options
        .get("path")
        .filter(|path| valid_xhttp_path(path))
        .ok_or_else(|| ClientError::transport(id, "HTTPS Stealth path is invalid", false))?;
    debug_assert!(!path.is_empty());
    let credential = transport.credential.as_ref().expect("validated credential");
    if !credential.secrets.is_empty() {
        return Err(ClientError::transport(
            id,
            "HTTPS Stealth credential contains unsupported secrets",
            false,
        ));
    }
    Ok(())
}

fn reject_unknown_options(
    transport: &SessionTransport,
    known: &[&str],
    name: &str,
) -> Result<(), ClientError> {
    if let Some(option) = transport
        .profile
        .options
        .keys()
        .find(|option| !known.contains(&option.as_str()))
    {
        return Err(ClientError::transport(
            &transport.profile.id,
            format!("unsupported {name} option {option}"),
            false,
        ));
    }
    Ok(())
}

fn require_options(
    transport: &SessionTransport,
    expected: &[(&str, &str)],
    name: &str,
) -> Result<(), ClientError> {
    for (key, expected) in expected {
        if transport.profile.options.get(*key).map(String::as_str) != Some(*expected) {
            return Err(ClientError::transport(
                &transport.profile.id,
                format!("unsupported {name} {key}"),
                false,
            ));
        }
    }
    Ok(())
}

fn validate_fingerprint<'a>(
    transport: &'a SessionTransport,
    name: &str,
) -> Result<&'a str, ClientError> {
    let fingerprint = transport
        .profile
        .options
        .get("fingerprint")
        .filter(|value| FINGERPRINTS.contains(&value.as_str()))
        .ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                format!("{name} fingerprint is invalid"),
                false,
            )
        })?;
    Ok(fingerprint)
}

fn valid_xhttp_path(path: &str) -> bool {
    (16..=128).contains(&path.len())
        && path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains("//")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
}

fn valid_hostname(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn required_secret<'a>(transport: &'a SessionTransport, key: &str) -> Result<&'a str, ClientError> {
    transport
        .credential
        .as_ref()
        .and_then(|credential| credential.secrets.get(key))
        .map(|value| value.expose_secret())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ClientError::transport(&transport.profile.id, format!("{key} is missing"), false)
        })
}

fn render_client_config(
    transport: &SessionTransport,
    server_address: IpAddr,
    socks_port: u16,
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let credential = transport.credential.as_ref().expect("validated credential");
    let options = &transport.profile.options;
    let (tag, user, stream_settings) = match transport.profile.protocol {
        TransportProtocol::VlessXtlsReality => (
            "ket-reality",
            json!({
                "id": credential.auth,
                "encryption": options["encryption"],
                "flow": options["flow"]
            }),
            json!({
                "network": options["transport"],
                "security": "reality",
                "realitySettings": {
                    "show": false,
                    "fingerprint": options["fingerprint"],
                    "serverName": transport.profile.tls_server_name,
                    "password": required_secret(transport, "reality_password")?,
                    "shortId": required_secret(transport, "reality_short_id")?,
                    "spiderX": "/"
                }
            }),
        ),
        TransportProtocol::Stealth => (
            "ket-stealth",
            json!({
                "id": credential.auth,
                "encryption": options["encryption"]
            }),
            json!({
                "network": options["transport"],
                "security": options["security"],
                "tlsSettings": {
                    "fingerprint": options["fingerprint"],
                    "serverName": transport.profile.tls_server_name,
                    "alpn": ["h2", "http/1.1"]
                },
                "xhttpSettings": {
                    "host": transport.profile.tls_server_name,
                    "path": options["path"],
                    "mode": options["mode"]
                }
            }),
        ),
        _ => unreachable!("transport validated above"),
    };
    let document = json!({
        "log": { "loglevel": "warning" },
        "inbounds": [{
            "tag": "ket-socks",
            "listen": "127.0.0.1",
            "port": socks_port,
            "protocol": "socks",
            "settings": { "auth": "noauth", "udp": true },
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls", "quic"],
                "routeOnly": true
            }
        }],
        "outbounds": [{
            "tag": tag,
            "protocol": "vless",
            "settings": {
                "vnext": [{
                    "address": server_address.to_string(),
                    "port": transport.profile.port,
                    "users": [user]
                }]
            },
            "streamSettings": stream_settings
        }]
    });
    serde_json::to_vec_pretty(&document).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            "failed to encode the Xray transport configuration",
            false,
        )
    })
}

pub(crate) async fn wait_for_socks(
    child: &mut Child,
    port: u16,
    startup_timeout: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let deadline = tokio::time::Instant::now() + startup_timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| {
            ClientError::transport(transport_id, "failed to inspect the Xray process", false)
        })? {
            return Err(ClientError::transport(
                transport_id,
                format!("Xray exited during startup ({status})"),
                true,
            ));
        }
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ClientError::transport(
                transport_id,
                "Xray local proxy startup timed out",
                true,
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn verify_tunnel_path(
    port: u16,
    target: &str,
    startup_timeout: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{port}")).map_err(|_| {
        ClientError::transport(
            transport_id,
            "failed to configure the local tunnel verification proxy",
            false,
        )
    })?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .timeout(startup_timeout)
        .build()
        .map_err(|_| {
            ClientError::transport(
                transport_id,
                "failed to initialize certificate verification",
                false,
            )
        })?;
    client
        .get(format!("https://{target}/"))
        .send()
        .await
        .map(|_| ())
        .map_err(|_| {
            ClientError::transport(
                transport_id,
                "the Xray path failed certificate-verified TLS",
                true,
            )
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, process::Command as StdCommand};

    use ket_core::{SecretString, TransportCredential, TransportProfile};
    use rand::Rng;

    use super::*;

    #[test]
    fn renders_a_strict_reality_client_without_downgrade_flags() {
        let transport = test_transport();
        let document =
            render_client_config(&transport, "203.0.113.9".parse().expect("test IP"), 10808)
                .expect("render config");
        let config: serde_json::Value = serde_json::from_slice(&document).expect("valid JSON");
        let reality = &config["outbounds"][0]["streamSettings"]["realitySettings"];
        assert_eq!(config["inbounds"][0]["protocol"], "socks");
        assert_eq!(
            config["outbounds"][0]["settings"]["vnext"][0]["address"],
            "203.0.113.9"
        );
        assert_eq!(reality["serverName"], "www.cloudflare.com");
        assert_eq!(reality["fingerprint"], "chrome");
        assert_eq!(reality["shortId"], "0123456789abcdef");
        assert!(reality.get("allowInsecure").is_none());
        assert!(!format!("{transport:?}").contains("0123456789abcdef"));
    }

    #[test]
    fn renders_a_strict_xhttp_tls_client_for_cdn_transport() {
        let transport = test_xhttp_transport();
        let document =
            render_client_config(&transport, "203.0.113.9".parse().expect("test IP"), 10808)
                .expect("render config");
        let config: serde_json::Value = serde_json::from_slice(&document).expect("valid JSON");
        let outbound = &config["outbounds"][0];
        let stream = &outbound["streamSettings"];
        assert_eq!(outbound["tag"], "ket-stealth");
        assert_eq!(outbound["settings"]["vnext"][0]["address"], "203.0.113.9");
        assert!(
            outbound["settings"]["vnext"][0]["users"][0]
                .get("flow")
                .is_none()
        );
        assert_eq!(stream["network"], "xhttp");
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["tlsSettings"]["serverName"], "stealth.example.test");
        assert_eq!(stream["xhttpSettings"]["mode"], "packet-up");
        assert_eq!(stream["xhttpSettings"]["path"], "/a1b2c3d4e5f6g7h8");
        assert!(stream["tlsSettings"].get("allowInsecure").is_none());
    }

    #[test]
    fn rejects_unknown_options_and_malformed_credentials() {
        let mut transport = test_transport();
        transport
            .profile
            .options
            .insert("allow_insecure".to_owned(), "true".to_owned());
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_transport();
        transport.credential.as_mut().expect("credential").auth = "not-a-uuid".into();
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_xhttp_transport();
        transport
            .credential
            .as_mut()
            .expect("credential")
            .secrets
            .insert("unexpected".to_owned(), SecretString::from("secret"));
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_xhttp_transport();
        transport
            .profile
            .options
            .insert("path".to_owned(), "/short".to_owned());
        assert!(validate_transport(&transport).is_err());
    }

    #[test]
    fn adapter_support_is_protocol_specific() {
        let transport = test_transport();
        let adapter = XrayAdapter::new(
            "xray",
            "tun2proxy",
            "/tmp/ket",
            "/tmp/ket/resolv.conf.state",
        );
        assert!(adapter.supports(&transport));
        assert!(adapter.supports(&test_xhttp_transport()));
    }

    #[test]
    fn pinned_xray_binary_accepts_rendered_config_when_supplied() {
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
        let binary = fs::canonicalize(binary).expect("resolve Xray test binary");
        for transport in [test_transport(), test_xhttp_transport()] {
            let document =
                render_client_config(&transport, "203.0.113.9".parse().expect("test IP"), 10808)
                    .expect("render config");
            let path = std::env::temp_dir().join(format!(
                "ket-xray-validation-{}.json",
                rand::thread_rng().r#gen::<u64>()
            ));
            fs::write(&path, document).expect("write test config");
            let status = StdCommand::new(&binary)
                .args(["run", "-test", "-c"])
                .arg(&path)
                .status()
                .expect("run Xray config validation");
            let _ = fs::remove_file(path);
            assert!(status.success());
        }
    }

    fn test_transport() -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: "vless-reality-primary".to_owned(),
                display_name: "VLESS + REALITY".to_owned(),
                protocol: TransportProtocol::VlessXtlsReality,
                endpoint: "vpn.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority: 5,
                tls_server_name: Some("www.cloudflare.com".to_owned()),
                options: BTreeMap::from([
                    ("encryption".to_owned(), "none".to_owned()),
                    ("fingerprint".to_owned(), "chrome".to_owned()),
                    ("flow".to_owned(), "xtls-rprx-vision".to_owned()),
                    ("transport".to_owned(), "raw".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("550e8400-e29b-41d4-a716-446655440000"),
                secrets: BTreeMap::from([
                    (
                        "reality_password".to_owned(),
                        SecretString::from("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                    ),
                    (
                        "reality_short_id".to_owned(),
                        SecretString::from("0123456789abcdef"),
                    ),
                ]),
            }),
        }
    }

    fn test_xhttp_transport() -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: "https-stealth-primary".to_owned(),
                display_name: "HTTPS Stealth".to_owned(),
                protocol: TransportProtocol::Stealth,
                endpoint: "stealth.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority: 1,
                tls_server_name: Some("stealth.example.test".to_owned()),
                options: BTreeMap::from([
                    ("encryption".to_owned(), "none".to_owned()),
                    ("fingerprint".to_owned(), "chrome".to_owned()),
                    ("mode".to_owned(), "packet-up".to_owned()),
                    ("path".to_owned(), "/a1b2c3d4e5f6g7h8".to_owned()),
                    ("security".to_owned(), "tls".to_owned()),
                    ("transport".to_owned(), "xhttp".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("550e8400-e29b-41d4-a716-446655440000"),
                secrets: BTreeMap::new(),
            }),
        }
    }
}
