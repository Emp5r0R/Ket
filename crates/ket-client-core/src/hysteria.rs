use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use ket_core::{Network, SecretString, SessionTransport, TransportProtocol};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    net::lookup_host,
    process::{Child, ChildStderr, Command},
    sync::{mpsc, watch},
    time::timeout,
};
use zeroize::Zeroize;

use crate::{
    ActiveTunnel, ClientError, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus,
    runtime::EphemeralConfig,
};

const KNOWN_OPTIONS: &[&str] = &["obfs", "gecko_min_packet_size", "gecko_max_packet_size"];
const KNOWN_SECRETS: &[&str] = &["obfs_password"];

#[derive(Clone, Debug)]
pub struct HysteriaTunSettings {
    pub interface_name: String,
    pub mtu: u16,
    pub ipv4_address: String,
    pub ipv6_address: String,
    pub enable_ipv6: bool,
}

impl Default for HysteriaTunSettings {
    fn default() -> Self {
        Self {
            interface_name: default_interface_name().to_owned(),
            mtu: 1400,
            ipv4_address: "100.100.100.101/30".to_owned(),
            ipv6_address: "2001::ffff:ffff:ffff:fff1/126".to_owned(),
            enable_ipv6: true,
        }
    }
}

#[cfg(target_os = "windows")]
fn default_interface_name() -> &'static str {
    "Ket"
}

#[cfg(not(target_os = "windows"))]
fn default_interface_name() -> &'static str {
    "ket0"
}

#[derive(Clone, Debug)]
pub struct Hysteria2Adapter {
    binary_path: PathBuf,
    runtime_dir: PathBuf,
    tun: HysteriaTunSettings,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl Hysteria2Adapter {
    pub fn new(
        binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
        tun: HysteriaTunSettings,
    ) -> Self {
        Self {
            binary_path: binary_path.into(),
            runtime_dir: runtime_dir.into(),
            tun,
            startup_timeout: Duration::from_secs(20),
            stop_timeout: Duration::from_secs(8),
        }
    }

    #[cfg(test)]
    fn with_timeouts(mut self, startup: Duration, stop: Duration) -> Self {
        self.startup_timeout = startup;
        self.stop_timeout = stop;
        self
    }
}

#[async_trait]
impl TransportAdapter for Hysteria2Adapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        transport.profile.protocol == TransportProtocol::Hysteria2
    }

    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
        validate_transport(transport)?;
        let started = Instant::now();
        let status = timeout(
            Duration::from_secs(5),
            Command::new(&self.binary_path)
                .arg("version")
                .env("HYSTERIA_DISABLE_UPDATE_CHECK", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await
        .map_err(|_| ClientError::transport(&transport.profile.id, "engine check timed out", true))?
        .map_err(|_| {
            ClientError::transport(
                &transport.profile.id,
                "Hysteria2 executable is unavailable",
                false,
            )
        })?;
        if !status.success() {
            return Err(ClientError::transport(
                &transport.profile.id,
                "Hysteria2 executable failed its version check",
                false,
            ));
        }

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
        addresses.sort();
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
        if probe.resolved_addresses.is_empty() {
            return Err(ClientError::transport(
                &transport.profile.id,
                "the server address was not resolved",
                true,
            ));
        }
        let started = Instant::now();
        let document = render_client_config(transport, &probe.resolved_addresses, &self.tun)?;
        let config = EphemeralConfig::create(&self.runtime_dir, "hysteria", document)
            .await
            .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;

        let mut child = Command::new(&self.binary_path)
            .arg("-c")
            .arg(config.path())
            .env("HYSTERIA_DISABLE_UPDATE_CHECK", "1")
            .env("HYSTERIA_LOG_FORMAT", "json")
            .env("HYSTERIA_LOG_LEVEL", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to launch the Hysteria2 engine",
                    false,
                )
            })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                "failed to capture Hysteria2 diagnostics",
                false,
            )
        })?;
        let mut lines = BufReader::new(stderr).lines();
        if let Err(error) = wait_until_ready(
            &mut child,
            &mut lines,
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(error);
        }
        drop(config);

        let (command_tx, command_rx) = mpsc::channel(1);
        let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
        tokio::spawn(drain_logs(lines));
        tokio::spawn(supervise_process(child, command_rx, status_tx));
        let tunnel = HysteriaTunnel {
            transport_id: transport.profile.id.clone(),
            command_tx,
            status_rx,
            stop_timeout: self.stop_timeout,
        };
        Ok(StartedTunnel {
            tunnel: Arc::new(tunnel),
            handshake_latency: started.elapsed(),
        })
    }
}

fn validate_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    if transport.profile.protocol != TransportProtocol::Hysteria2
        || transport.profile.network != Network::Udp
    {
        return Err(ClientError::transport(
            id,
            "profile is not a Hysteria2 UDP transport",
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
    if transport
        .profile
        .tls_server_name
        .as_ref()
        .is_none_or(|name| name.trim().is_empty())
    {
        return Err(ClientError::transport(
            id,
            "TLS server name is required",
            false,
        ));
    }
    let credential = transport
        .credential
        .as_ref()
        .ok_or_else(|| ClientError::transport(id, "transport credential is missing", false))?;
    if credential.auth.is_empty() {
        return Err(ClientError::transport(
            id,
            "transport credential is empty",
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
            format!("unsupported Hysteria2 option {option}"),
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
            format!("unsupported Hysteria2 credential {secret}"),
            false,
        ));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientConfig<'a> {
    server: String,
    auth: &'a SecretString,
    tls: TlsConfig<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    obfs: Option<ObfsConfig<'a>>,
    quic: QuicConfig,
    fast_open: bool,
    tun: TunConfig,
}

#[derive(Serialize)]
struct TlsConfig<'a> {
    sni: &'a str,
}

#[derive(Serialize)]
struct ObfsConfig<'a> {
    r#type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    salamander: Option<ObfsPassword<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gecko: Option<GeckoConfig<'a>>,
}

#[derive(Serialize)]
struct ObfsPassword<'a> {
    password: &'a SecretString,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeckoConfig<'a> {
    password: &'a SecretString,
    min_packet_size: u16,
    max_packet_size: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuicConfig {
    max_idle_timeout: &'static str,
    keep_alive_period: &'static str,
}

#[derive(Serialize)]
struct TunConfig {
    name: String,
    mtu: u16,
    timeout: &'static str,
    address: TunAddress,
    route: TunRoute,
}

#[derive(Serialize)]
struct TunAddress {
    ipv4: String,
    ipv6: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TunRoute {
    strict: bool,
    ipv4: Vec<String>,
    ipv6: Vec<String>,
    ipv4_exclude: Vec<String>,
    ipv6_exclude: Vec<String>,
}

fn render_client_config(
    transport: &SessionTransport,
    addresses: &[SocketAddr],
    settings: &HysteriaTunSettings,
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let credential = transport.credential.as_ref().expect("validated credential");
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .expect("validated SNI");
    let obfs = match transport.profile.options.get("obfs").map(String::as_str) {
        None | Some("none") => {
            if credential.secrets.contains_key("obfs_password") {
                return Err(ClientError::transport(
                    &transport.profile.id,
                    "obfuscation password was supplied without an obfuscation mode",
                    false,
                ));
            }
            None
        }
        Some("salamander") => {
            let password = obfs_password(transport)?;
            Some(ObfsConfig {
                r#type: "salamander",
                salamander: Some(ObfsPassword { password }),
                gecko: None,
            })
        }
        Some("gecko") => {
            let password = obfs_password(transport)?;
            let min_packet_size = packet_size(transport, "gecko_min_packet_size", 512)?;
            let max_packet_size = packet_size(transport, "gecko_max_packet_size", 1200)?;
            if min_packet_size > max_packet_size || max_packet_size > 2048 {
                return Err(ClientError::transport(
                    &transport.profile.id,
                    "Gecko packet size bounds are invalid",
                    false,
                ));
            }
            Some(ObfsConfig {
                r#type: "gecko",
                salamander: None,
                gecko: Some(GeckoConfig {
                    password,
                    min_packet_size,
                    max_packet_size,
                }),
            })
        }
        Some(_) => {
            return Err(ClientError::transport(
                &transport.profile.id,
                "unsupported Hysteria2 obfuscation mode",
                false,
            ));
        }
    };

    let mut ipv4_exclude = BTreeSet::new();
    let mut ipv6_exclude = BTreeSet::new();
    for address in addresses {
        match address.ip() {
            IpAddr::V4(address) => {
                ipv4_exclude.insert(format!("{address}/32"));
            }
            IpAddr::V6(address) => {
                ipv6_exclude.insert(format!("{address}/128"));
            }
        }
    }
    if !settings.enable_ipv6 {
        ipv6_exclude.clear();
        ipv6_exclude.insert("::/0".to_owned());
    }

    let server_host = if matches!(
        transport.profile.endpoint.to_ascii_lowercase().as_str(),
        "hysteria2" | "hy2"
    ) {
        match addresses[0].ip() {
            IpAddr::V4(address) => address.to_string(),
            IpAddr::V6(address) => format!("[{address}]"),
        }
    } else {
        match transport.profile.endpoint.parse::<IpAddr>() {
            Ok(IpAddr::V6(address)) => format!("[{address}]"),
            _ => transport.profile.endpoint.clone(),
        }
    };
    let config = ClientConfig {
        server: format!("{server_host}:{}", transport.profile.port),
        auth: &credential.auth,
        tls: TlsConfig { sni },
        obfs,
        quic: QuicConfig {
            max_idle_timeout: "30s",
            keep_alive_period: "10s",
        },
        fast_open: false,
        tun: TunConfig {
            name: settings.interface_name.clone(),
            mtu: settings.mtu,
            timeout: "5m",
            address: TunAddress {
                ipv4: settings.ipv4_address.clone(),
                ipv6: settings.ipv6_address.clone(),
            },
            route: TunRoute {
                strict: true,
                ipv4: vec!["0.0.0.0/0".to_owned()],
                ipv6: if settings.enable_ipv6 {
                    vec!["::/0".to_owned()]
                } else {
                    Vec::new()
                },
                ipv4_exclude: ipv4_exclude.into_iter().collect(),
                ipv6_exclude: ipv6_exclude.into_iter().collect(),
            },
        },
    };
    serde_json::to_vec_pretty(&config).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            "failed to encode Hysteria2 configuration",
            false,
        )
    })
}

fn obfs_password(transport: &SessionTransport) -> Result<&SecretString, ClientError> {
    transport
        .credential
        .as_ref()
        .and_then(|credential| credential.secrets.get("obfs_password"))
        .filter(|password| !password.is_empty())
        .ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                "obfuscation password is missing",
                false,
            )
        })
}

fn packet_size(transport: &SessionTransport, key: &str, default: u16) -> Result<u16, ClientError> {
    transport
        .profile
        .options
        .get(key)
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| {
            ClientError::transport(&transport.profile.id, format!("{key} is invalid"), false)
        })
        .map(|value| value.unwrap_or(default))
}

async fn wait_until_ready(
    child: &mut Child,
    lines: &mut Lines<BufReader<ChildStderr>>,
    startup_timeout: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let deadline = tokio::time::Instant::now() + startup_timeout;
    let mut connected = false;
    let mut tun_ready = false;
    let mut diagnostic = None;
    loop {
        if connected && tun_ready {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|_| {
            ClientError::transport(transport_id, "failed to inspect transport process", false)
        })? {
            return Err(ClientError::transport(
                transport_id,
                diagnostic.unwrap_or_else(|| format!("Hysteria2 exited during startup ({status})")),
                true,
            ));
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(ClientError::transport(
                transport_id,
                diagnostic.unwrap_or_else(|| "Hysteria2 connection timed out".to_owned()),
                true,
            ));
        }
        let wait = (deadline - now).min(Duration::from_millis(250));
        match timeout(wait, lines.next_line()).await {
            Ok(Ok(Some(mut line))) => {
                connected |= line.contains("connected to server");
                tun_ready |= line.contains("TUN listening");
                diagnostic = classify_diagnostic(&line).or(diagnostic);
                line.zeroize();
            }
            Ok(Ok(None)) => {
                return Err(ClientError::transport(
                    transport_id,
                    "Hysteria2 diagnostics closed during startup",
                    true,
                ));
            }
            Ok(Err(_)) => {
                return Err(ClientError::transport(
                    transport_id,
                    "failed to read Hysteria2 diagnostics",
                    true,
                ));
            }
            Err(_) => {}
        }
    }
}

fn classify_diagnostic(line: &str) -> Option<String> {
    let normalized = line.to_ascii_lowercase();
    if normalized.contains("permission denied") || normalized.contains("operation not permitted") {
        Some("TUN setup requires elevated network permissions".to_owned())
    } else if normalized.contains("authentication failed")
        || normalized.contains("authentication error")
    {
        Some("the server rejected the transport credential".to_owned())
    } else if normalized.contains("certificate") || normalized.contains("tls verification") {
        Some("server certificate verification failed".to_owned())
    } else if normalized.contains("no recent network activity")
        || normalized.contains("connect error")
    {
        Some("the Hysteria2 server did not respond".to_owned())
    } else if normalized.contains("network is unreachable") {
        Some("the server network is unreachable".to_owned())
    } else {
        None
    }
}

async fn drain_logs(mut lines: Lines<BufReader<ChildStderr>>) {
    while let Ok(Some(mut line)) = lines.next_line().await {
        line.zeroize();
    }
}

enum ProcessCommand {
    Stop,
}

async fn supervise_process(
    mut child: Child,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    tokio::select! {
        result = child.wait() => {
            let message = match result {
                Ok(exit) => format!("Hysteria2 exited unexpectedly ({exit})"),
                Err(_) => "failed to wait for Hysteria2".to_owned(),
            };
            status.send_replace(TunnelStatus::Failed(message));
        }
        command = commands.recv() => {
            if matches!(command, Some(ProcessCommand::Stop)) {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            status.send_replace(TunnelStatus::Stopped);
        }
    }
}

struct HysteriaTunnel {
    transport_id: String,
    command_tx: mpsc::Sender<ProcessCommand>,
    status_rx: watch::Receiver<TunnelStatus>,
    stop_timeout: Duration,
}

#[async_trait]
impl ActiveTunnel for HysteriaTunnel {
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
    use std::{collections::BTreeMap, fs, net::Ipv4Addr};

    use ket_core::{TransportCredential, TransportProfile};
    use rand::{Rng, distributions::Alphanumeric};

    use super::*;

    #[test]
    fn renderer_enables_strict_tun_routes_and_keeps_tls_verification() {
        let transport = test_transport("gecko");
        let addresses = [SocketAddr::from((Ipv4Addr::new(203, 0, 113, 8), 443))];
        let bytes = render_client_config(&transport, &addresses, &HysteriaTunSettings::default())
            .expect("config should render");
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(document["server"], "vpn.example.test:443");
        assert_eq!(document["tls"]["sni"], "cdn.example.test");
        assert!(document["tls"].get("insecure").is_none());
        assert_eq!(document["obfs"]["type"], "gecko");
        assert_eq!(document["tun"]["route"]["strict"], true);
        assert_eq!(document["tun"]["route"]["ipv4Exclude"][0], "203.0.113.8/32");
        assert_eq!(document["fastOpen"], false);
    }

    #[test]
    fn renderer_rejects_unknown_options_and_missing_credentials() {
        let mut transport = test_transport("salamander");
        transport
            .profile
            .options
            .insert("insecure".to_owned(), "true".to_owned());
        let address = ["203.0.113.8:443".parse().unwrap()];
        assert!(
            render_client_config(&transport, &address, &HysteriaTunSettings::default()).is_err()
        );

        transport.profile.options.remove("insecure");
        transport.credential = None;
        assert!(
            render_client_config(&transport, &address, &HysteriaTunSettings::default()).is_err()
        );
    }

    fn test_transport(obfs: &str) -> SessionTransport {
        let mut options = BTreeMap::new();
        options.insert("obfs".to_owned(), obfs.to_owned());
        options.insert("gecko_min_packet_size".to_owned(), "512".to_owned());
        options.insert("gecko_max_packet_size".to_owned(), "1200".to_owned());
        let mut secrets = BTreeMap::new();
        secrets.insert(
            "obfs_password".to_owned(),
            SecretString::from("test-obfuscation-secret-at-least-32-characters"),
        );
        SessionTransport {
            profile: TransportProfile {
                id: "hy2-primary".to_owned(),
                display_name: "Hysteria 2".to_owned(),
                protocol: TransportProtocol::Hysteria2,
                endpoint: "vpn.example.test".to_owned(),
                port: 443,
                network: Network::Udp,
                priority: 10,
                tls_server_name: Some("cdn.example.test".to_owned()),
                options,
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("test-data-plane-token"),
                secrets,
            }),
        }
    }

    #[test]
    fn test_timeout_override_is_available_for_process_tests() {
        let adapter = Hysteria2Adapter::new(
            "hysteria",
            std::env::temp_dir(),
            HysteriaTunSettings::default(),
        )
        .with_timeouts(Duration::from_millis(1), Duration::from_millis(1));
        assert_eq!(adapter.startup_timeout, Duration::from_millis(1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn adapter_supervises_process_and_removes_ephemeral_config() {
        use std::os::unix::fs::PermissionsExt;

        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(12)
            .map(char::from)
            .collect();
        let root = std::env::temp_dir().join(format!("ket-hysteria-test-{suffix}"));
        let runtime = root.join("runtime");
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("fake-hysteria");
        fs::write(
            &binary,
            "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then exit 0; fi\necho '{\"msg\":\"connected to server\"}' >&2\necho '{\"msg\":\"TUN listening\"}' >&2\nexec sleep 300\n",
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let adapter = Hysteria2Adapter::new(&binary, &runtime, HysteriaTunSettings::default())
            .with_timeouts(Duration::from_secs(2), Duration::from_secs(2));
        let mut transport = test_transport("gecko");
        transport.profile.endpoint = "localhost".to_owned();

        let probe = adapter.probe(&transport).await.unwrap();
        let started = adapter.connect(&transport, &probe).await.unwrap();
        assert!(matches!(
            *started.tunnel.status().borrow(),
            TunnelStatus::Connected
        ));
        assert_eq!(fs::read_dir(&runtime).unwrap().count(), 0);
        started.tunnel.stop().await.unwrap();
        assert!(matches!(
            *started.tunnel.status().borrow(),
            TunnelStatus::Stopped
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
