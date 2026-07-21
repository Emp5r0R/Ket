use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ket_core::{Network, SessionTransport, TransportProtocol};
use rand::{Rng, distributions::Alphanumeric};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, lookup_host},
    process::{Child, Command},
    time::{sleep, timeout},
};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    ClientError, ProbeReport, StartedTunnel, TransportAdapter,
    full_route::{reserve_proxy_port, stop_child, supervise_pair, wait_until_pair_stable},
    runtime::EphemeralConfig,
};

const OPTIONS: &[&str] = &[
    "auth_mode",
    "cipher",
    "remote_cert_tls",
    "tls_crypt",
    "tls_minimum",
    "transport",
];
const SECRETS: &[&str] = &[
    "ca_certificate_pem_b64",
    "stunnel_ca_certificate_pem_b64",
    "tls_crypt_key_b64",
    "username",
];
const MAX_MANAGEMENT_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct OpenVpnStunnelAdapter {
    openvpn_binary_path: PathBuf,
    stunnel_binary_path: PathBuf,
    runtime_dir: PathBuf,
    startup_timeout: Duration,
    stop_timeout: Duration,
}

impl OpenVpnStunnelAdapter {
    pub fn new(
        openvpn_binary_path: impl Into<PathBuf>,
        stunnel_binary_path: impl Into<PathBuf>,
        runtime_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            openvpn_binary_path: openvpn_binary_path.into(),
            stunnel_binary_path: stunnel_binary_path.into(),
            runtime_dir: runtime_dir.into(),
            startup_timeout: Duration::from_secs(30),
            stop_timeout: Duration::from_secs(10),
        }
    }
}

#[async_trait]
impl TransportAdapter for OpenVpnStunnelAdapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        transport.profile.protocol == TransportProtocol::OpenVpnStunnel
    }

    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
        validate_transport(transport)?;
        let started = Instant::now();
        check_binary(
            &self.openvpn_binary_path,
            &["--version"],
            "OpenVPN",
            &transport.profile.id,
        )
        .await?;
        check_binary(
            &self.stunnel_binary_path,
            &["-version"],
            "stunnel",
            &transport.profile.id,
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
        let carrier_port = reserve_proxy_port(&transport.profile.id).await?;
        let management_port = reserve_proxy_port(&transport.profile.id).await?;
        if carrier_port == management_port {
            return Err(ClientError::transport(
                &transport.profile.id,
                "failed to allocate distinct local control ports",
                true,
            ));
        }
        let mut management_password = Zeroizing::new(
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(48)
                .map(char::from)
                .collect::<String>(),
        );
        let management_password_file = EphemeralConfig::create(
            &self.runtime_dir,
            "openvpn-management",
            format!("{}\n", management_password.as_str()).into_bytes(),
        )
        .await
        .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;

        let stunnel_ca = decode_material(
            transport,
            "stunnel_ca_certificate_pem_b64",
            "-----BEGIN CERTIFICATE-----",
            "-----END CERTIFICATE-----",
        )?;
        let stunnel_ca_file = EphemeralConfig::create(
            &self.runtime_dir,
            "openvpn-stunnel-ca",
            stunnel_ca.as_bytes().to_vec(),
        )
        .await
        .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;
        let stunnel_document =
            render_stunnel_config(transport, server.ip(), carrier_port, stunnel_ca_file.path())?;
        let stunnel_config =
            EphemeralConfig::create(&self.runtime_dir, "openvpn-stunnel", stunnel_document)
                .await
                .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;
        let mut carrier_command = Command::new(&self.stunnel_binary_path);
        carrier_command
            .arg(stunnel_config.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(parent) = self.stunnel_binary_path.parent() {
            let modules = parent.join("ossl-modules");
            if modules.is_dir() {
                carrier_command.env("OPENSSL_MODULES", modules);
            }
        }
        let mut carrier = carrier_command.spawn().map_err(|_| {
            ClientError::transport(
                &transport.profile.id,
                "failed to launch the stunnel carrier",
                false,
            )
        })?;
        if let Err(error) = wait_for_listener(
            &mut carrier,
            carrier_port,
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut carrier).await;
            return Err(error);
        }

        let openvpn_document = render_openvpn_config(
            transport,
            carrier_port,
            management_port,
            management_password_file.path(),
            &probe.resolved_addresses,
        )?;
        let openvpn_config =
            EphemeralConfig::create(&self.runtime_dir, "openvpn-client", openvpn_document)
                .await
                .map_err(|message| ClientError::transport(&transport.profile.id, message, false))?;
        let mut engine_command = Command::new(&self.openvpn_binary_path);
        engine_command
            .arg("--config")
            .arg(openvpn_config.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(parent) = self.openvpn_binary_path.parent() {
            engine_command.env("OPENSSL_MODULES", parent);
        }
        let mut engine = engine_command.spawn().map_err(|_| {
            ClientError::transport(
                &transport.profile.id,
                "failed to launch the OpenVPN engine",
                false,
            )
        })?;
        if let Err(error) = wait_for_openvpn(
            &mut engine,
            management_port,
            management_password.as_str(),
            self.startup_timeout,
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut engine).await;
            stop_child(&mut carrier).await;
            return Err(error);
        }
        if let Err(error) = wait_until_pair_stable(
            &mut engine,
            "OpenVPN",
            &mut carrier,
            "stunnel",
            Duration::from_millis(1200),
            &transport.profile.id,
        )
        .await
        {
            stop_child(&mut engine).await;
            stop_child(&mut carrier).await;
            return Err(error);
        }
        management_password.zeroize();
        drop(openvpn_config);
        drop(management_password_file);
        drop(stunnel_config);
        drop(stunnel_ca_file);

        Ok(StartedTunnel {
            tunnel: supervise_pair(
                transport.profile.id.clone(),
                "OpenVPN",
                engine,
                "stunnel",
                carrier,
                self.stop_timeout,
            ),
            handshake_latency: started.elapsed(),
        })
    }
}

async fn check_binary(
    path: &Path,
    arguments: &[&str],
    name: &str,
    transport_id: &str,
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
    if transport.profile.protocol != TransportProtocol::OpenVpnStunnel
        || transport.profile.network != Network::Tcp
        || transport.profile.port == 0
        || !valid_endpoint(&transport.profile.endpoint)
    {
        return Err(ClientError::transport(
            id,
            "profile is not a valid OpenVPN stunnel transport",
            false,
        ));
    }
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .filter(|name| valid_hostname(name) && name.parse::<IpAddr>().is_err())
        .ok_or_else(|| ClientError::transport(id, "OpenVPN TLS server name is invalid", false))?;
    if sni != transport.profile.endpoint && transport.profile.endpoint.parse::<IpAddr>().is_err() {
        return Err(ClientError::transport(
            id,
            "OpenVPN endpoint and TLS identity do not match",
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
            "OpenVPN profile contains unsupported options",
            false,
        ));
    }
    for (key, expected) in [
        ("auth_mode", "session_token"),
        ("cipher", "aes_256_gcm"),
        ("remote_cert_tls", "server"),
        ("tls_crypt", "v1"),
        ("tls_minimum", "1.2"),
        ("transport", "stunnel_tls"),
    ] {
        if option(transport, key)? != expected {
            return Err(ClientError::transport(
                id,
                format!("unsupported OpenVPN {key}"),
                false,
            ));
        }
    }
    let credential = transport
        .credential
        .as_ref()
        .ok_or_else(|| ClientError::transport(id, "OpenVPN credential is missing", false))?;
    if credential.secrets.len() != SECRETS.len()
        || credential
            .secrets
            .keys()
            .any(|key| !SECRETS.contains(&key.as_str()))
    {
        return Err(ClientError::transport(
            id,
            "OpenVPN credential contains unsupported secrets",
            false,
        ));
    }
    let username = required_secret(transport, "username")?;
    let password = credential.auth.expose_secret();
    if username.len() != 12
        || !username.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || password.len() != 44
        || !password.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || !password.starts_with(username)
    {
        return Err(ClientError::transport(
            id,
            "OpenVPN scoped credential is invalid",
            false,
        ));
    }
    decode_material(
        transport,
        "ca_certificate_pem_b64",
        "-----BEGIN CERTIFICATE-----",
        "-----END CERTIFICATE-----",
    )?;
    decode_material(
        transport,
        "stunnel_ca_certificate_pem_b64",
        "-----BEGIN CERTIFICATE-----",
        "-----END CERTIFICATE-----",
    )?;
    decode_material(
        transport,
        "tls_crypt_key_b64",
        "-----BEGIN OpenVPN Static key V1-----",
        "-----END OpenVPN Static key V1-----",
    )?;
    Ok(())
}

fn render_stunnel_config(
    transport: &SessionTransport,
    server_address: IpAddr,
    local_port: u16,
    ca_path: &Path,
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .expect("validated TLS server name");
    let server = authority(server_address, transport.profile.port);
    let ca_path = config_path(ca_path, &transport.profile.id)?;
    Ok(format!(
        "foreground = yes\nsyslog = no\ndebug = notice\nclient = yes\nsslVersionMin = TLSv1.2\n\n[ket-openvpn]\naccept = 127.0.0.1:{local_port}\nconnect = {server}\nverifyChain = yes\nCAfile = {ca_path}\ncheckHost = {sni}\nsni = {sni}\n"
    )
    .into_bytes())
}

fn render_openvpn_config(
    transport: &SessionTransport,
    carrier_port: u16,
    management_port: u16,
    management_password_path: &Path,
    server_addresses: &[SocketAddr],
) -> Result<Vec<u8>, ClientError> {
    validate_transport(transport)?;
    let credential = transport.credential.as_ref().expect("validated credential");
    let username = required_secret(transport, "username")?;
    let ca = decode_material(
        transport,
        "ca_certificate_pem_b64",
        "-----BEGIN CERTIFICATE-----",
        "-----END CERTIFICATE-----",
    )?;
    let tls_crypt = decode_material(
        transport,
        "tls_crypt_key_b64",
        "-----BEGIN OpenVPN Static key V1-----",
        "-----END OpenVPN Static key V1-----",
    )?;
    let sni = transport
        .profile
        .tls_server_name
        .as_deref()
        .expect("validated TLS server name");
    let management_password_path =
        quote_openvpn_path(management_password_path, &transport.profile.id)?;
    let mut document = format!(
        "client\ndev tun\nproto tcp-client\nremote 127.0.0.1 {carrier_port}\nnobind\nconnect-retry 2 5\nconnect-retry-max 3\nconnect-timeout 10\nresolv-retry 0\nremote-cert-tls server\nverify-x509-name {sni} name\ntls-version-min 1.2\ntls-cert-profile preferred\ndata-ciphers AES-256-GCM:AES-128-GCM:CHACHA20-POLY1305\ndata-ciphers-fallback AES-256-GCM\nauth SHA256\nallow-compression no\nauth-retry none\nredirect-gateway def1 bypass-dhcp\nblock-ipv6\npersist-key\npersist-tun\nmanagement 127.0.0.1 {management_port} {management_password_path}\nmanagement-log-cache 50\nverb 3\nmute 10\n"
    );
    #[cfg(windows)]
    document.push_str("windows-driver wintun\nblock-outside-dns\n");
    let addresses = server_addresses
        .iter()
        .map(SocketAddr::ip)
        .collect::<BTreeSet<_>>();
    for address in addresses {
        match address {
            IpAddr::V4(address) => {
                document.push_str(&format!("route {address} 255.255.255.255 net_gateway\n"));
            }
            IpAddr::V6(address) => {
                document.push_str(&format!("route-ipv6 {address}/128 net_gateway\n"));
            }
        }
    }
    document.push_str("<ca>\n");
    document.push_str(ca.trim());
    document.push_str("\n</ca>\n<tls-crypt>\n");
    document.push_str(tls_crypt.trim());
    document.push_str("\n</tls-crypt>\n<auth-user-pass>\n");
    document.push_str(username);
    document.push('\n');
    document.push_str(credential.auth.expose_secret());
    document.push_str("\n</auth-user-pass>\n");
    Ok(document.into_bytes())
}

async fn wait_for_listener(
    child: &mut Child,
    port: u16,
    wait: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    timeout(wait, async {
        loop {
            if let Some(status) = child.try_wait().map_err(|_| {
                ClientError::transport(transport_id, "failed to inspect stunnel", false)
            })? {
                return Err(ClientError::transport(
                    transport_id,
                    format!("stunnel exited during startup ({status})"),
                    true,
                ));
            }
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| ClientError::transport(transport_id, "stunnel startup timed out", true))?
}

async fn wait_for_openvpn(
    child: &mut Child,
    management_port: u16,
    management_password: &str,
    wait: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    timeout(wait, async {
        loop {
            if let Some(status) = child.try_wait().map_err(|_| {
                ClientError::transport(transport_id, "failed to inspect OpenVPN", false)
            })? {
                return Err(ClientError::transport(
                    transport_id,
                    format!("OpenVPN exited during startup ({status})"),
                    true,
                ));
            }
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", management_port)).await {
                match openvpn_state(&mut stream, management_password).await {
                    Ok(state)
                        if state.lines().any(|line| {
                            let fields = line.split(',').collect::<Vec<_>>();
                            fields.get(1) == Some(&"CONNECTED") && fields.get(2) == Some(&"SUCCESS")
                        }) =>
                    {
                        return Ok(());
                    }
                    Ok(_) | Err(_) => {}
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .map_err(|_| ClientError::transport(transport_id, "OpenVPN startup timed out", true))?
}

async fn openvpn_state(stream: &mut TcpStream, password: &str) -> Result<String, std::io::Error> {
    read_until(stream, b"ENTER PASSWORD:").await?;
    stream.write_all(password.as_bytes()).await?;
    stream.write_all(b"\nstate\nquit\n").await?;
    stream.shutdown().await?;
    let mut response = Vec::new();
    stream
        .take(MAX_MANAGEMENT_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut response)
        .await?;
    if response.len() > MAX_MANAGEMENT_RESPONSE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "management response is too large",
        ));
    }
    String::from_utf8(response)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 response"))
}

async fn read_until(stream: &mut TcpStream, marker: &[u8]) -> Result<(), std::io::Error> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 512];
    while response.len() <= MAX_MANAGEMENT_RESPONSE_BYTES {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
        if response
            .windows(marker.len())
            .any(|window| window == marker)
        {
            return Ok(());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "management password prompt is missing",
    ))
}

fn decode_material(
    transport: &SessionTransport,
    key: &str,
    begin: &str,
    end: &str,
) -> Result<Zeroizing<String>, ClientError> {
    let encoded = required_secret(transport, key)?;
    let mut decoded = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            format!("OpenVPN secret {key} is not base64"),
            false,
        )
    })?);
    if decoded.is_empty() || decoded.len() > 3 * 1024 {
        return Err(ClientError::transport(
            &transport.profile.id,
            format!("OpenVPN secret {key} has an invalid size"),
            false,
        ));
    }
    let material = String::from_utf8(std::mem::take(&mut *decoded)).map_err(|_| {
        ClientError::transport(
            &transport.profile.id,
            format!("OpenVPN secret {key} is not UTF-8"),
            false,
        )
    })?;
    let material = Zeroizing::new(material);
    let trimmed = material.trim();
    if !trimmed.starts_with(begin)
        || !trimmed.ends_with(end)
        || trimmed.contains('\0')
        || trimmed.lines().any(|line| line.len() > 256)
    {
        return Err(ClientError::transport(
            &transport.profile.id,
            format!("OpenVPN secret {key} contains invalid key material"),
            false,
        ));
    }
    Ok(material)
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
                format!("OpenVPN option {key} is missing"),
                false,
            )
        })
}

fn required_secret<'a>(transport: &'a SessionTransport, key: &str) -> Result<&'a str, ClientError> {
    transport
        .credential
        .as_ref()
        .and_then(|credential| credential.secrets.get(key))
        .map(|secret| secret.expose_secret())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ClientError::transport(
                &transport.profile.id,
                format!("OpenVPN secret {key} is missing"),
                false,
            )
        })
}

fn authority(address: IpAddr, port: u16) -> String {
    match address {
        IpAddr::V4(address) => format!("{address}:{port}"),
        IpAddr::V6(address) => format!("[{address}]:{port}"),
    }
}

fn config_path(path: &Path, id: &str) -> Result<String, ClientError> {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.is_empty() || path.contains(['\n', '\r', '\0']) {
        return Err(ClientError::transport(
            id,
            "private runtime path is invalid",
            false,
        ));
    }
    Ok(path)
}

fn quote_openvpn_path(path: &Path, id: &str) -> Result<String, ClientError> {
    let path = config_path(path, id)?;
    Ok(format!("\"{}\"", path.replace('"', "\\\"")))
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
    use std::collections::BTreeMap;

    use ket_core::{SecretString, TransportCredential, TransportProfile};

    use super::*;

    #[test]
    fn renders_verified_stunnel_and_hardened_openvpn_configs() {
        let transport = test_transport();
        let stunnel = String::from_utf8(
            render_stunnel_config(
                &transport,
                "203.0.113.9".parse().unwrap(),
                11940,
                Path::new("/run/ket/stunnel-ca.pem"),
            )
            .unwrap(),
        )
        .unwrap();
        for expected in [
            "verifyChain = yes",
            "checkHost = openvpn.example.test",
            "sni = openvpn.example.test",
            "connect = 203.0.113.9:443",
            "sslVersionMin = TLSv1.2",
        ] {
            assert!(stunnel.contains(expected), "missing {expected}");
        }

        let openvpn = String::from_utf8(
            render_openvpn_config(
                &transport,
                11940,
                17500,
                Path::new("/run/ket/management-password"),
                &["203.0.113.9:443".parse().unwrap()],
            )
            .unwrap(),
        )
        .unwrap();
        for expected in [
            "remote 127.0.0.1 11940",
            "remote-cert-tls server",
            "verify-x509-name openvpn.example.test name",
            "tls-version-min 1.2",
            "allow-compression no",
            "auth-retry none",
            "redirect-gateway def1 bypass-dhcp",
            "block-ipv6",
            "route 203.0.113.9 255.255.255.255 net_gateway",
            "<tls-crypt>",
            "<auth-user-pass>",
        ] {
            assert!(openvpn.contains(expected), "missing {expected}");
        }
        assert!(!format!("{transport:?}").contains("AbCdEf123456ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"));
    }

    #[test]
    fn rejects_downgrades_unknown_fields_and_mismatched_credentials() {
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
            .insert("tls_minimum".to_owned(), "1.0".to_owned());
        assert!(validate_transport(&transport).is_err());
        let mut transport = test_transport();
        transport.credential.as_mut().unwrap().auth =
            SecretString::from("Z23456789012ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
        assert!(validate_transport(&transport).is_err());
    }

    fn test_transport() -> SessionTransport {
        let pem = |begin: &str, body: &str, end: &str| {
            STANDARD.encode(format!("{begin}\n{body}\n{end}\n"))
        };
        SessionTransport {
            profile: TransportProfile {
                id: "openvpn-stunnel-primary".to_owned(),
                display_name: "OpenVPN TLS".to_owned(),
                protocol: TransportProtocol::OpenVpnStunnel,
                endpoint: "openvpn.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority: 8,
                tls_server_name: Some("openvpn.example.test".to_owned()),
                options: BTreeMap::from([
                    ("auth_mode".to_owned(), "session_token".to_owned()),
                    ("cipher".to_owned(), "aes_256_gcm".to_owned()),
                    ("remote_cert_tls".to_owned(), "server".to_owned()),
                    ("tls_crypt".to_owned(), "v1".to_owned()),
                    ("tls_minimum".to_owned(), "1.2".to_owned()),
                    ("transport".to_owned(), "stunnel_tls".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("AbCdEf123456ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"),
                secrets: BTreeMap::from([
                    (
                        "ca_certificate_pem_b64".to_owned(),
                        SecretString::from(pem(
                            "-----BEGIN CERTIFICATE-----",
                            "dGVzdC1jYQ==",
                            "-----END CERTIFICATE-----",
                        )),
                    ),
                    (
                        "stunnel_ca_certificate_pem_b64".to_owned(),
                        SecretString::from(pem(
                            "-----BEGIN CERTIFICATE-----",
                            "dGVzdC1vdXRlci1jYQ==",
                            "-----END CERTIFICATE-----",
                        )),
                    ),
                    (
                        "tls_crypt_key_b64".to_owned(),
                        SecretString::from(pem(
                            "-----BEGIN OpenVPN Static key V1-----",
                            "dGVzdC10bHMta2V5",
                            "-----END OpenVPN Static key V1-----",
                        )),
                    ),
                    ("username".to_owned(), SecretString::from("AbCdEf123456")),
                ]),
            }),
        }
    }
}
