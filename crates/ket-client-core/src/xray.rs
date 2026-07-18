use std::{
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use ket_core::{Network, SessionTransport, TransportProtocol};
use serde_json::json;
use tokio::{
    net::{TcpListener, TcpStream, lookup_host},
    process::{Child, Command},
    sync::{mpsc, watch},
    time::{sleep, timeout},
};

use crate::{
    ActiveTunnel, ClientError, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus,
    runtime::EphemeralConfig,
};

const KNOWN_OPTIONS: &[&str] = &["encryption", "fingerprint", "flow", "transport"];
const KNOWN_SECRETS: &[&str] = &["reality_password", "reality_short_id"];
const FINGERPRINTS: &[&str] = &[
    "chrome", "firefox", "safari", "ios", "android", "edge", "random",
];

#[derive(Clone, Debug)]
pub struct XrayRealityAdapter {
    xray_binary_path: PathBuf,
    bridge_binary_path: PathBuf,
    runtime_dir: PathBuf,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl XrayRealityAdapter {
    pub fn new(
        xray_binary_path: impl Into<PathBuf>,
        bridge_binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            xray_binary_path: xray_binary_path.into(),
            bridge_binary_path: bridge_binary_path.into(),
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(20),
            stop_timeout: Duration::from_secs(8),
        }
    }
}

#[async_trait]
impl TransportAdapter for XrayRealityAdapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        transport.profile.protocol == TransportProtocol::VlessXtlsReality
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
        check_binary(
            &self.bridge_binary_path,
            &["--version"],
            &transport.profile.id,
            "tun2proxy",
        )
        .await?;
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
        let socks_port = reserve_port(&transport.profile.id).await?;
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
                "Xray rejected the Reality configuration",
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
            .expect("validated Reality SNI");
        if let Err(error) =
            verify_reality_path(socks_port, sni, self.startup_timeout, &transport.profile.id).await
        {
            stop_child(&mut xray).await;
            return Err(error);
        }

        let mut bridge_command = Command::new(&self.bridge_binary_path);
        bridge_command
            .arg("--setup")
            .arg("--proxy")
            .arg(format!("socks5://127.0.0.1:{socks_port}"))
            .arg("--dns")
            .arg("virtual")
            .arg("--verbosity")
            .arg("error")
            .arg("--exit-on-fatal-error")
            .arg("--ipv6-enabled");
        for address in &probe.resolved_addresses {
            bridge_command.arg("--bypass").arg(address.ip().to_string());
        }
        let mut bridge = bridge_command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to launch the full-route bridge",
                    false,
                )
            })?;
        if let Err(error) = wait_for_bridge(
            &mut xray,
            &mut bridge,
            Duration::from_millis(1200),
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut bridge).await;
            stop_child(&mut xray).await;
            return Err(error);
        }
        drop(config);

        let (command_tx, command_rx) = mpsc::channel(1);
        let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
        tokio::spawn(supervise_processes(xray, bridge, command_rx, status_tx));
        Ok(StartedTunnel {
            tunnel: Arc::new(XrayTunnel {
                transport_id: transport.profile.id.clone(),
                command_tx,
                status_rx,
                stop_timeout: self.stop_timeout,
            }),
            handshake_latency: started.elapsed(),
        })
    }
}

async fn check_binary(
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
    if transport.profile.protocol != TransportProtocol::VlessXtlsReality
        || transport.profile.network != Network::Tcp
    {
        return Err(ClientError::transport(
            id,
            "profile is not a VLESS + REALITY TCP transport",
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
        .ok_or_else(|| ClientError::transport(id, "Reality server name is invalid", false))?;
    if sni.parse::<IpAddr>().is_ok() {
        return Err(ClientError::transport(
            id,
            "Reality server name must be a hostname",
            false,
        ));
    }
    if let Some(option) = transport
        .profile
        .options
        .keys()
        .find(|option| !KNOWN_OPTIONS.contains(&option.as_str()))
    {
        return Err(ClientError::transport(
            id,
            format!("unsupported VLESS + REALITY option {option}"),
            false,
        ));
    }
    for (key, expected) in [
        ("encryption", "none"),
        ("flow", "xtls-rprx-vision"),
        ("transport", "raw"),
    ] {
        if transport.profile.options.get(key).map(String::as_str) != Some(expected) {
            return Err(ClientError::transport(
                id,
                format!("unsupported VLESS + REALITY {key}"),
                false,
            ));
        }
    }
    let fingerprint = transport
        .profile
        .options
        .get("fingerprint")
        .filter(|value| FINGERPRINTS.contains(&value.as_str()))
        .ok_or_else(|| ClientError::transport(id, "Reality fingerprint is invalid", false))?;
    debug_assert!(!fingerprint.is_empty());
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
    if let Some(secret) = credential
        .secrets
        .keys()
        .find(|secret| !KNOWN_SECRETS.contains(&secret.as_str()))
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
            "tag": "ket-reality",
            "protocol": "vless",
            "settings": {
                "vnext": [{
                    "address": server_address.to_string(),
                    "port": transport.profile.port,
                    "users": [{
                        "id": credential.auth,
                        "encryption": options["encryption"],
                        "flow": options["flow"]
                    }]
                }]
            },
            "streamSettings": {
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
            }
        }]
    });
    serde_json::to_vec_pretty(&document).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            "failed to encode the Xray Reality configuration",
            false,
        )
    })
}

async fn reserve_port(transport_id: &str) -> Result<u16, ClientError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|_| {
        ClientError::transport(transport_id, "failed to reserve a local proxy port", true)
    })?;
    Ok(listener
        .local_addr()
        .expect("bound listener has an address")
        .port())
}

async fn wait_for_socks(
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

async fn verify_reality_path(
    port: u16,
    target: &str,
    startup_timeout: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{port}")).map_err(|_| {
        ClientError::transport(
            transport_id,
            "failed to configure the local REALITY verification proxy",
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
                "the VLESS + REALITY path failed certificate-verified TLS",
                true,
            )
        })
}

async fn wait_for_bridge(
    xray: &mut Child,
    bridge: &mut Child,
    settle_time: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let deadline = tokio::time::Instant::now() + settle_time;
    loop {
        if xray
            .try_wait()
            .map_err(|_| {
                ClientError::transport(transport_id, "failed to inspect the Xray process", false)
            })?
            .is_some()
        {
            return Err(ClientError::transport(
                transport_id,
                "Xray stopped while enabling the full-route tunnel",
                true,
            ));
        }
        if bridge
            .try_wait()
            .map_err(|_| {
                ClientError::transport(transport_id, "failed to inspect the route bridge", false)
            })?
            .is_some()
        {
            return Err(ClientError::transport(
                transport_id,
                "the full-route bridge could not configure this system",
                false,
            ));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn stop_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

enum ProcessCommand {
    Stop,
}

async fn supervise_processes(
    mut xray: Child,
    mut bridge: Child,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    tokio::select! {
        result = xray.wait() => {
            stop_child(&mut bridge).await;
            let message = result.map_or_else(
                |_| "failed to wait for Xray".to_owned(),
                |exit| format!("Xray exited unexpectedly ({exit})"),
            );
            status.send_replace(TunnelStatus::Failed(message));
        }
        result = bridge.wait() => {
            stop_child(&mut xray).await;
            let message = result.map_or_else(
                |_| "failed to wait for the full-route bridge".to_owned(),
                |exit| format!("full-route bridge exited unexpectedly ({exit})"),
            );
            status.send_replace(TunnelStatus::Failed(message));
        }
        command = commands.recv() => {
            if matches!(command, Some(ProcessCommand::Stop)) {
                stop_child(&mut bridge).await;
                stop_child(&mut xray).await;
            }
            status.send_replace(TunnelStatus::Stopped);
        }
    }
}

struct XrayTunnel {
    transport_id: String,
    command_tx: mpsc::Sender<ProcessCommand>,
    status_rx: watch::Receiver<TunnelStatus>,
    stop_timeout: Duration,
}

#[async_trait]
impl ActiveTunnel for XrayTunnel {
    fn transport_id(&self) -> &str {
        &self.transport_id
    }

    fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    async fn stop(&self) -> Result<(), ClientError> {
        if !matches!(*self.status_rx.borrow(), TunnelStatus::Connected) {
            return Ok(());
        }
        self.command_tx
            .send(ProcessCommand::Stop)
            .await
            .map_err(|_| {
                ClientError::transport(
                    &self.transport_id,
                    "transport supervisor is unavailable",
                    true,
                )
            })?;
        let mut status = self.status_rx.clone();
        timeout(self.stop_timeout, async {
            while matches!(*status.borrow(), TunnelStatus::Connected) {
                if status.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .map_err(|_| {
            ClientError::transport(&self.transport_id, "transport shutdown timed out", true)
        })?;
        Ok(())
    }
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
    }

    #[test]
    fn adapter_support_is_protocol_specific() {
        let transport = test_transport();
        let adapter = XrayRealityAdapter::new("xray", "tun2proxy", "/tmp/ket");
        assert!(adapter.supports(&transport));
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
        let document = render_client_config(
            &test_transport(),
            "203.0.113.9".parse().expect("test IP"),
            10808,
        )
        .expect("render config");
        let path = std::env::temp_dir().join(format!(
            "ket-xray-validation-{}.json",
            rand::thread_rng().r#gen::<u64>()
        ));
        fs::write(&path, document).expect("write test config");
        let status = StdCommand::new(binary)
            .args(["run", "-test", "-c"])
            .arg(&path)
            .status()
            .expect("run Xray config validation");
        let _ = fs::remove_file(path);
        assert!(status.success());
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
}
