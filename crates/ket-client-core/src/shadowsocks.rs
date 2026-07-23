use std::{
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ket_core::{Network, SecretString, SessionTransport, TransportProtocol};
use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, lookup_host},
    process::{Child, Command},
    time::{sleep, timeout},
};
use zeroize::Zeroizing;

use crate::{
    ClientError, ProbeReport, StartedTunnel, TransportAdapter,
    full_route::{
        FullRouteBridge, reserve_proxy_port, stop_bridge, stop_child, supervise, wait_until_stable,
    },
    runtime::EphemeralConfig,
};

const METHOD: &str = "2022-blake3-aes-256-gcm";
const OPTIONS: &[&str] = &["method", "mode", "port_allocation"];
const PATH_CHECK_ADDRESS: [u8; 4] = [1, 1, 1, 1];
const PATH_CHECK_PORT: u16 = 443;

#[derive(Clone, Debug)]
pub struct Shadowsocks2022Adapter {
    binary_path: PathBuf,
    bridge: FullRouteBridge,
    runtime_dir: PathBuf,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl Shadowsocks2022Adapter {
    pub fn new(
        binary_path: impl Into<PathBuf>,
        bridge_binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
        dns_state_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            binary_path: binary_path.into(),
            bridge: FullRouteBridge::new(bridge_binary_path, dns_state_path),
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(20),
            stop_timeout: Duration::from_secs(8),
        }
    }
}

#[async_trait]
impl TransportAdapter for Shadowsocks2022Adapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        transport.profile.protocol == TransportProtocol::Shadowsocks2022
    }

    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
        validate_transport(transport)?;
        let started = Instant::now();
        check_binary(&self.binary_path, &transport.profile.id).await?;
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
        let config = EphemeralConfig::create(&self.runtime_dir, "shadowsocks", document)
            .await
            .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;

        let mut engine = Command::new(&self.binary_path)
            .args(["--config"])
            .arg(config.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    &transport.profile.id,
                    "failed to launch the Shadowsocks engine",
                    false,
                )
            })?;
        if let Err(error) = wait_for_socks(
            &mut engine,
            socks_port,
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut engine).await;
            return Err(error);
        }
        if let Err(error) =
            verify_tunnel_path(socks_port, self.startup_timeout, &transport.profile.id).await
        {
            stop_child(&mut engine).await;
            return Err(error);
        }

        let mut bridge =
            self.bridge
                .start(socks_port, &probe.resolved_addresses, &transport.profile.id)?;
        if let Err(error) = wait_until_stable(
            &mut engine,
            "Shadowsocks",
            &mut bridge,
            Duration::from_millis(1200),
            &transport.profile.id,
        )
        .await
        {
            stop_bridge(&mut bridge).await;
            stop_child(&mut engine).await;
            return Err(error);
        }
        drop(config);

        Ok(StartedTunnel {
            tunnel: supervise(
                transport.profile.id.clone(),
                "Shadowsocks",
                engine,
                bridge,
                self.stop_timeout,
            ),
            handshake_latency: started.elapsed(),
        })
    }
}

async fn check_binary(path: &PathBuf, transport_id: &str) -> Result<(), ClientError> {
    let status = timeout(
        Duration::from_secs(5),
        Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| ClientError::transport(transport_id, "Shadowsocks check timed out", true))?
    .map_err(|_| {
        ClientError::transport(transport_id, "Shadowsocks executable is unavailable", false)
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(ClientError::transport(
            transport_id,
            "Shadowsocks executable failed its version check",
            false,
        ))
    }
}

fn validate_transport(transport: &SessionTransport) -> Result<(), ClientError> {
    let id = &transport.profile.id;
    if transport.profile.protocol != TransportProtocol::Shadowsocks2022
        || transport.profile.network != Network::TcpAndUdp
    {
        return Err(ClientError::transport(
            id,
            "profile is not a Shadowsocks 2022 TCP+UDP transport",
            false,
        ));
    }
    if transport.profile.endpoint.trim().is_empty()
        || transport.profile.endpoint.len() > 253
        || transport.profile.endpoint.contains("://")
        || transport.profile.endpoint.contains('/')
        || transport.profile.endpoint.contains('\\')
        || transport.profile.endpoint.chars().any(char::is_whitespace)
        || transport.profile.port == 0
    {
        return Err(ClientError::transport(
            id,
            "server endpoint is invalid",
            false,
        ));
    }
    if transport.profile.tls_server_name.is_some() {
        return Err(ClientError::transport(
            id,
            "Shadowsocks 2022 must not declare a TLS server name",
            false,
        ));
    }
    if let Some(option) = transport
        .profile
        .options
        .keys()
        .find(|option| !OPTIONS.contains(&option.as_str()))
    {
        return Err(ClientError::transport(
            id,
            format!("unsupported Shadowsocks 2022 option {option}"),
            false,
        ));
    }
    for (key, expected) in [
        ("method", METHOD),
        ("mode", "tcp_and_udp"),
        ("port_allocation", "lease_slot"),
    ] {
        if transport.profile.options.get(key).map(String::as_str) != Some(expected) {
            return Err(ClientError::transport(
                id,
                format!("unsupported Shadowsocks 2022 {key}"),
                false,
            ));
        }
    }
    let credential = transport
        .credential
        .as_ref()
        .ok_or_else(|| ClientError::transport(id, "transport credential is missing", false))?;
    if !credential.secrets.is_empty() {
        return Err(ClientError::transport(
            id,
            "Shadowsocks 2022 credential contains unsupported secrets",
            false,
        ));
    }
    let key = Zeroizing::new(
        STANDARD
            .decode(credential.auth.expose_secret())
            .map_err(|_| ClientError::transport(id, "Shadowsocks 2022 key is invalid", false))?,
    );
    if key.len() != 32 {
        return Err(ClientError::transport(
            id,
            "Shadowsocks 2022 key must contain 32 bytes",
            false,
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct ClientConfig<'a> {
    server: String,
    server_port: u16,
    local_address: &'static str,
    local_port: u16,
    password: &'a SecretString,
    method: &'static str,
    mode: &'static str,
    timeout: u16,
    udp_timeout: u16,
    no_delay: bool,
    keep_alive: u16,
}

fn render_client_config(
    transport: &SessionTransport,
    server_address: IpAddr,
    socks_port: u16,
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let credential = transport.credential.as_ref().expect("validated credential");
    serde_json::to_vec_pretty(&ClientConfig {
        server: server_address.to_string(),
        server_port: transport.profile.port,
        local_address: "127.0.0.1",
        local_port: socks_port,
        password: &credential.auth,
        method: METHOD,
        mode: "tcp_and_udp",
        timeout: 300,
        udp_timeout: 300,
        no_delay: true,
        keep_alive: 30,
    })
    .map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            "failed to encode the Shadowsocks configuration",
            false,
        )
    })
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
            ClientError::transport(
                transport_id,
                "failed to inspect the Shadowsocks process",
                false,
            )
        })? {
            return Err(ClientError::transport(
                transport_id,
                format!("Shadowsocks exited during startup ({status})"),
                true,
            ));
        }
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ClientError::transport(
                transport_id,
                "Shadowsocks local proxy startup timed out",
                true,
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn verify_tunnel_path(
    port: u16,
    startup_timeout: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    timeout(startup_timeout, async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        stream.write_all(&[5, 1, 0]).await?;
        let mut greeting = [0_u8; 2];
        stream.read_exact(&mut greeting).await?;
        if greeting != [5, 0] {
            return Err(std::io::Error::other("SOCKS authentication failed"));
        }
        let mut request = [0_u8; 10];
        request[..4].copy_from_slice(&[5, 1, 0, 1]);
        request[4..8].copy_from_slice(&PATH_CHECK_ADDRESS);
        request[8..].copy_from_slice(&PATH_CHECK_PORT.to_be_bytes());
        stream.write_all(&request).await?;
        let mut response = [0_u8; 4];
        stream.read_exact(&mut response).await?;
        if response[0] != 5 || response[1] != 0 {
            return Err(std::io::Error::other("SOCKS connect failed"));
        }
        let address_bytes = match response[3] {
            1 => 4,
            4 => 16,
            3 => {
                let mut length = [0_u8; 1];
                stream.read_exact(&mut length).await?;
                usize::from(length[0])
            }
            _ => return Err(std::io::Error::other("SOCKS response is invalid")),
        };
        let mut remainder = vec![0_u8; address_bytes + 2];
        stream.read_exact(&mut remainder).await?;
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| ClientError::transport(transport_id, "Shadowsocks path check timed out", true))?
    .map_err(|_| {
        ClientError::transport(
            transport_id,
            "the Shadowsocks path could not reach its verification endpoint",
            true,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf, process::Command as StdCommand};

    use ket_core::{TransportCredential, TransportProfile};
    use rand::Rng;

    use super::*;

    #[test]
    fn renders_a_strict_aead_2022_tcp_udp_client() {
        let transport = test_transport();
        let document =
            render_client_config(&transport, "203.0.113.9".parse().unwrap(), 10808).unwrap();
        let config: serde_json::Value = serde_json::from_slice(&document).unwrap();

        assert_eq!(config["server"], "203.0.113.9");
        assert_eq!(config["server_port"], 20_000);
        assert_eq!(config["local_address"], "127.0.0.1");
        assert_eq!(config["local_port"], 10808);
        assert_eq!(config["method"], METHOD);
        assert_eq!(config["mode"], "tcp_and_udp");
        assert_eq!(config["no_delay"], true);
        assert!(!format!("{transport:?}").contains(test_key()));
    }

    #[test]
    fn rejects_unknown_options_downgrades_and_malformed_keys() {
        let mut transport = test_transport();
        transport
            .profile
            .options
            .insert("plugin".to_owned(), "plain".to_owned());
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_transport();
        transport
            .profile
            .options
            .insert("method".to_owned(), "chacha20-ietf-poly1305".to_owned());
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_transport();
        transport.profile.tls_server_name = Some("unexpected.example".to_owned());
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_transport();
        transport.credential.as_mut().unwrap().auth = "not-base64".into();
        assert!(validate_transport(&transport).is_err());

        let mut transport = test_transport();
        transport
            .credential
            .as_mut()
            .unwrap()
            .secrets
            .insert("plugin_key".to_owned(), "secret".into());
        assert!(validate_transport(&transport).is_err());
    }

    #[test]
    fn adapter_support_is_protocol_specific() {
        assert!(
            Shadowsocks2022Adapter::new(
                "sslocal",
                "tun2proxy",
                "/tmp/ket",
                "/tmp/ket/resolv.conf.state",
            )
            .supports(&test_transport())
        );
    }

    #[test]
    fn pinned_sslocal_accepts_rendered_config_when_supplied() {
        let Some(binary) = std::env::var_os("KET_TEST_SHADOWSOCKS_LOCAL_BINARY") else {
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
        let binary = fs::canonicalize(binary).expect("resolve Shadowsocks test binary");
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve SOCKS port")
            .local_addr()
            .unwrap()
            .port();
        let document =
            render_client_config(&test_transport(), "203.0.113.9".parse().unwrap(), port).unwrap();
        let path = std::env::temp_dir().join(format!(
            "ket-shadowsocks-validation-{}.json",
            rand::thread_rng().r#gen::<u64>()
        ));
        fs::write(&path, document).expect("write test config");
        let mut child = StdCommand::new(binary)
            .args(["--config"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start pinned sslocal");
        let ready = (0..50).any(|_| {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                true
            } else {
                std::thread::sleep(Duration::from_millis(50));
                false
            }
        });
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(path);
        assert!(ready, "pinned sslocal rejected the rendered config");
    }

    fn test_key() -> &'static str {
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    }

    fn test_transport() -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: "shadowsocks-2022-primary".to_owned(),
                display_name: "Shadowsocks 2022".to_owned(),
                protocol: TransportProtocol::Shadowsocks2022,
                endpoint: "vpn.example.test".to_owned(),
                port: 20_000,
                network: Network::TcpAndUdp,
                priority: 15,
                tls_server_name: None,
                options: BTreeMap::from([
                    ("method".to_owned(), METHOD.to_owned()),
                    ("mode".to_owned(), "tcp_and_udp".to_owned()),
                    ("port_allocation".to_owned(), "lease_slot".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from(test_key()),
                secrets: BTreeMap::new(),
            }),
        }
    }
}
