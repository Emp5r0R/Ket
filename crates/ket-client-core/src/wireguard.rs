use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ket_core::{Network, SessionTransport, TransportProtocol};
use serde_json::json;
use tokio::{
    net::{UdpSocket, lookup_host},
    process::{Child, Command},
    time::{sleep, timeout},
};

use crate::{
    ClientError, ProbeReport, StartedTunnel, TransportAdapter,
    full_route::{
        FullRouteBridge, reserve_proxy_port, stop_bridge, stop_child, supervise_with_carrier,
        wait_until_three_stable,
    },
    runtime::EphemeralConfig,
    xray::{check_binary, verify_tunnel_path, wait_for_socks},
};

const OPTIONS: &[&str] = &[
    "address_allocation",
    "allowed_ips",
    "client_address",
    "keepalive_seconds",
    "mtu",
    "path_prefix",
    "remote_address",
    "transport",
];
const SECRETS: &[&str] = &["preshared_key", "server_public_key"];

#[derive(Clone, Debug)]
pub struct WireGuardTlsAdapter {
    xray_binary_path: PathBuf,
    wstunnel_binary_path: PathBuf,
    bridge: FullRouteBridge,
    runtime_dir: PathBuf,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl WireGuardTlsAdapter {
    pub fn new(
        xray_binary_path: impl Into<PathBuf>,
        wstunnel_binary_path: impl Into<PathBuf>,
        bridge_binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
        dns_state_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            xray_binary_path: xray_binary_path.into(),
            wstunnel_binary_path: wstunnel_binary_path.into(),
            bridge: FullRouteBridge::new(bridge_binary_path, dns_state_path),
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(25),
            stop_timeout: Duration::from_secs(8),
        }
    }
}

#[async_trait]
impl TransportAdapter for WireGuardTlsAdapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        transport.profile.protocol == TransportProtocol::WireGuard
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
            &self.wstunnel_binary_path,
            &["--version"],
            &transport.profile.id,
            "wstunnel",
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
        let mut addresses = resolved.collect::<Vec<_>>();
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
        let wireguard_port = reserve_udp_port(&transport.profile.id).await?;
        let document = render_xray_config(transport, wireguard_port, socks_port)?;
        let config = EphemeralConfig::create(&self.runtime_dir, "wireguard", document)
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
                    "failed to validate the WireGuard Xray configuration",
                    false,
                )
            })?;
        if !validation.success() {
            return Err(ClientError::transport(
                &transport.profile.id,
                "Xray rejected the WireGuard configuration",
                false,
            ));
        }

        let arguments = wstunnel_arguments(transport, server.ip(), wireguard_port)?;
        let mut carrier = Command::new(&self.wstunnel_binary_path)
            .args(arguments)
            .env("NO_COLOR", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to launch the WebSocket TLS carrier",
                    false,
                )
            })?;
        if let Err(error) = wait_for_process(&mut carrier, "wstunnel", &transport.profile.id).await
        {
            stop_child(&mut carrier).await;
            return Err(error);
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
                    "failed to launch the WireGuard Xray engine",
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
            stop_child(&mut carrier).await;
            return Err(error);
        }
        if let Err(error) = verify_tunnel_path(
            socks_port,
            "one.one.one.one",
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut xray).await;
            stop_child(&mut carrier).await;
            return Err(error);
        }

        let mut bridge =
            self.bridge
                .start(socks_port, &probe.resolved_addresses, &transport.profile.id)?;
        if let Err(error) = wait_until_three_stable(
            &mut xray,
            "Xray WireGuard",
            &mut carrier,
            "wstunnel",
            &mut bridge,
            Duration::from_millis(1200),
            &transport.profile.id,
        )
        .await
        {
            stop_bridge(&mut bridge).await;
            stop_child(&mut xray).await;
            stop_child(&mut carrier).await;
            return Err(error);
        }
        drop(config);

        Ok(StartedTunnel {
            tunnel: supervise_with_carrier(
                transport.profile.id.clone(),
                "Xray WireGuard",
                xray,
                "wstunnel",
                carrier,
                bridge,
                self.stop_timeout,
            ),
            handshake_latency: started.elapsed(),
        })
    }
}

fn validate_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    if transport.profile.protocol != TransportProtocol::WireGuard
        || transport.profile.network != Network::Tcp
        || transport.profile.port == 0
        || !valid_endpoint(&transport.profile.endpoint)
    {
        return Err(ClientError::transport(
            id,
            "profile is not a valid WireGuard TLS transport",
            false,
        ));
    }
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .filter(|name| valid_hostname(name))
        .ok_or_else(|| ClientError::transport(id, "WireGuard TLS server name is invalid", false))?;
    if sni.parse::<IpAddr>().is_ok() {
        return Err(ClientError::transport(
            id,
            "WireGuard TLS server name must be a hostname",
            false,
        ));
    }
    if transport
        .profile
        .options
        .keys()
        .any(|key| !OPTIONS.contains(&key.as_str()))
    {
        return Err(ClientError::transport(
            id,
            "WireGuard TLS profile contains unsupported options",
            false,
        ));
    }
    for (key, expected) in [
        ("address_allocation", "lease_slot"),
        ("allowed_ips", "0.0.0.0/0"),
        ("keepalive_seconds", "25"),
        ("mtu", "1280"),
        ("transport", "websocket_tls"),
    ] {
        if option(transport, key)? != expected {
            return Err(ClientError::transport(
                id,
                format!("unsupported WireGuard TLS {key}"),
                false,
            ));
        }
    }
    let address = option(transport, "client_address")?
        .parse::<Ipv4Addr>()
        .map_err(|_| ClientError::transport(id, "WireGuard client address is invalid", false))?;
    let octets = address.octets();
    let host = (u16::from(octets[2]) << 8) | u16::from(octets[3]);
    if octets[..2] != [10, 66] || !(2..=65_534).contains(&host) {
        return Err(ClientError::transport(
            id,
            "WireGuard client address is outside the lease pool",
            false,
        ));
    }
    let prefix = option(transport, "path_prefix")?;
    if !(16..=96).contains(&prefix.len())
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ClientError::transport(
            id,
            "WireGuard WebSocket path prefix is invalid",
            false,
        ));
    }
    validate_remote_address(option(transport, "remote_address")?, id)?;
    let credential = transport
        .credential
        .as_ref()
        .ok_or_else(|| ClientError::transport(id, "WireGuard credential is missing", false))?;
    if credential.secrets.len() != SECRETS.len()
        || credential
            .secrets
            .keys()
            .any(|key| !SECRETS.contains(&key.as_str()))
    {
        return Err(ClientError::transport(
            id,
            "WireGuard credential contains unsupported secrets",
            false,
        ));
    }
    validate_key(credential.auth.expose_secret(), "WireGuard private key", id)?;
    validate_key(
        required_secret(transport, "preshared_key")?,
        "WireGuard preshared key",
        id,
    )?;
    validate_key(
        required_secret(transport, "server_public_key")?,
        "WireGuard server public key",
        id,
    )
}

fn render_xray_config(
    transport: &SessionTransport,
    wireguard_port: u16,
    socks_port: u16,
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let credential = transport.credential.as_ref().expect("validated credential");
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
            "tag": "ket-wireguard",
            "protocol": "wireguard",
            "settings": {
                "secretKey": credential.auth,
                "address": [format!("{}/32", option(transport, "client_address")?)],
                "noKernelTun": true,
                "mtu": 1280,
                "domainStrategy": "ForceIP",
                "peers": [{
                    "publicKey": required_secret(transport, "server_public_key")?,
                    "preSharedKey": required_secret(transport, "preshared_key")?,
                    "endpoint": format!("127.0.0.1:{wireguard_port}"),
                    "allowedIPs": ["0.0.0.0/0"],
                    "keepAlive": 25
                }]
            }
        }]
    });
    serde_json::to_vec_pretty(&document).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            "failed to encode the WireGuard Xray configuration",
            false,
        )
    })
}

fn wstunnel_arguments(
    transport: &SessionTransport,
    server_address: IpAddr,
    local_port: u16,
) -> Result<Vec<String>, ClientError> {
    validate_transport(transport)?;
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .expect("validated TLS server name");
    let host = match server_address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    Ok(vec![
        "client".to_owned(),
        "--no-color".to_owned(),
        "--log-lvl".to_owned(),
        "WARN".to_owned(),
        "--tls-sni-override".to_owned(),
        sni.to_owned(),
        "--tls-verify-certificate".to_owned(),
        "--http-upgrade-path-prefix".to_owned(),
        option(transport, "path_prefix")?.to_owned(),
        "--http-headers".to_owned(),
        format!("Host: {sni}"),
        "--local-to-remote".to_owned(),
        format!(
            "udp://127.0.0.1:{local_port}:{}?timeout_sec=0",
            option(transport, "remote_address")?
        ),
        format!("wss://{host}:{}", transport.profile.port),
    ])
}

async fn reserve_udp_port(transport_id: &str) -> Result<u16, ClientError> {
    let socket = UdpSocket::bind(("127.0.0.1", 0)).await.map_err(|_| {
        ClientError::transport(
            transport_id,
            "failed to reserve a local WireGuard port",
            true,
        )
    })?;
    Ok(socket.local_addr().expect("bound UDP socket").port())
}

async fn wait_for_process(
    child: &mut Child,
    name: &str,
    transport_id: &str,
) -> Result<(), ClientError> {
    for _ in 0..5 {
        if let Some(status) = child.try_wait().map_err(|_| {
            ClientError::transport(transport_id, format!("failed to inspect {name}"), false)
        })? {
            return Err(ClientError::transport(
                transport_id,
                format!("{name} exited during startup ({status})"),
                true,
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

fn option<'a>(transport: &'a SessionTransport, key: &str) -> Result<&'a str, ClientError> {
    transport
        .profile
        .options
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                format!("WireGuard option {key} is missing"),
                false,
            )
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
            ClientError::transport(
                &transport.profile.id,
                format!("WireGuard secret {key} is missing"),
                false,
            )
        })
}

fn validate_key(value: &str, name: &str, id: &str) -> Result<(), ClientError> {
    if STANDARD
        .decode(value)
        .ok()
        .is_none_or(|decoded| decoded.len() != 32)
    {
        return Err(ClientError::transport(
            id,
            format!("{name} is invalid"),
            false,
        ));
    }
    Ok(())
}

fn validate_remote_address(value: &str, id: &str) -> Result<(), ClientError> {
    let Some((host, port)) = value.rsplit_once(':') else {
        return Err(ClientError::transport(
            id,
            "WireGuard remote address is invalid",
            false,
        ));
    };
    if !valid_hostname(host) || port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
        return Err(ClientError::transport(
            id,
            "WireGuard remote address is invalid",
            false,
        ));
    }
    Ok(())
}

fn valid_endpoint(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok() || valid_hostname(value)
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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf, process::Command as StdCommand};

    use ket_core::{SecretString, TransportCredential, TransportProfile};
    use rand::Rng;

    use super::*;

    #[test]
    fn renders_strict_wireguard_and_verified_wstunnel_configuration() {
        let transport = test_transport();
        let rendered = render_xray_config(&transport, 51821, 10808).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&rendered).unwrap();
        let settings = &document["outbounds"][0]["settings"];
        assert_eq!(settings["address"][0], "10.66.0.2/32");
        assert_eq!(settings["peers"][0]["endpoint"], "127.0.0.1:51821");
        assert_eq!(settings["peers"][0]["allowedIPs"][0], "0.0.0.0/0");
        assert_eq!(settings["noKernelTun"], true);

        let args = wstunnel_arguments(&transport, "203.0.113.9".parse().unwrap(), 51821).unwrap();
        assert!(
            args.windows(2)
                .any(|args| args == ["--tls-verify-certificate", "--http-upgrade-path-prefix"])
        );
        assert!(
            args.windows(2)
                .any(|args| args == ["--http-headers", "Host: wg.example.test"])
        );
        assert!(args.iter().any(|arg| arg == "wss://203.0.113.9:443"));
        assert!(!format!("{transport:?}").contains("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="));
    }

    #[test]
    fn rejects_downgrades_unknown_fields_and_malformed_keys() {
        let mut transport = test_transport();
        transport
            .profile
            .options
            .insert("allow_insecure".to_owned(), "true".to_owned());
        assert!(validate_transport(&transport).is_err());
        let mut transport = test_transport();
        transport
            .profile
            .options
            .insert("transport".to_owned(), "websocket".to_owned());
        assert!(validate_transport(&transport).is_err());
        let mut transport = test_transport();
        transport.credential.as_mut().unwrap().auth = SecretString::from("invalid");
        assert!(validate_transport(&transport).is_err());
    }

    #[test]
    fn pinned_xray_binary_accepts_wireguard_config_when_supplied() {
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
        let document = render_xray_config(&test_transport(), 51821, 10808).unwrap();
        let path = std::env::temp_dir().join(format!(
            "ket-wireguard-xray-validation-{}.json",
            rand::thread_rng().r#gen::<u64>()
        ));
        fs::write(&path, document).expect("write WireGuard Xray config");
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
                id: "wireguard-tls-primary".to_owned(),
                display_name: "WireGuard TLS".to_owned(),
                protocol: TransportProtocol::WireGuard,
                endpoint: "wg.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority: 2,
                tls_server_name: Some("wg.example.test".to_owned()),
                options: BTreeMap::from([
                    ("address_allocation".to_owned(), "lease_slot".to_owned()),
                    ("allowed_ips".to_owned(), "0.0.0.0/0".to_owned()),
                    ("client_address".to_owned(), "10.66.0.2".to_owned()),
                    ("keepalive_seconds".to_owned(), "25".to_owned()),
                    ("mtu".to_owned(), "1280".to_owned()),
                    ("path_prefix".to_owned(), "ket-wireguard-test".to_owned()),
                    (
                        "remote_address".to_owned(),
                        "wireguard-agent:51820".to_owned(),
                    ),
                    ("transport".to_owned(), "websocket_tls".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="),
                secrets: BTreeMap::from([
                    (
                        "preshared_key".to_owned(),
                        SecretString::from("AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI="),
                    ),
                    (
                        "server_public_key".to_owned(),
                        SecretString::from("AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM="),
                    ),
                ]),
            }),
        }
    }
}
