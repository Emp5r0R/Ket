use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ket_core::{Network, NodeLocation, TransportProfile, TransportProtocol};
use url::{Host, Url};

const MAX_NODE_TEXT_CHARS: usize = 128;
const MAX_PUBLIC_URL_CHARS: usize = 2_048;
const MAX_TRANSPORTS: usize = 32;
const MAX_TRANSPORT_ID_CHARS: usize = 128;
const MAX_ENDPOINT_CHARS: usize = 253;
const MAX_OPTION_ENTRIES: usize = 32;
const MAX_OPTION_KEY_CHARS: usize = 64;
const MAX_OPTION_VALUE_CHARS: usize = 2_048;
const MAX_SHADOWSOCKS_MANAGER_SERVERS: u32 = 1_500;
const MAX_OPENVPN_MATERIAL_BYTES: u64 = 3 * 1024;

#[derive(Clone)]
pub struct ServerConfig {
    pub bind_address: SocketAddr,
    pub state_path: PathBuf,
    pub admin_token: String,
    pub public_url: String,
    pub node_id: String,
    pub node_name: String,
    pub location: NodeLocation,
    pub max_sessions: u32,
    pub session_ttl: Duration,
    pub transports: Vec<TransportProfile>,
    pub hysteria: Option<HysteriaConfig>,
    pub openvpn: Option<OpenVpnConfig>,
    pub shadowsocks: Option<ShadowsocksConfig>,
    pub wireguard: Option<WireGuardConfig>,
    pub xray: Option<XrayConfig>,
}

#[derive(Clone)]
pub struct HysteriaConfig {
    pub transport_id: String,
    pub runtime_config_path: PathBuf,
    pub listen: String,
    pub public_host: String,
    pub public_port: u16,
    pub sni: String,
    pub tls_cert_path: String,
    pub tls_key_path: String,
    pub auth_url: String,
    pub stats_url: String,
    pub stats_secret: String,
    pub masquerade_url: String,
    pub obfuscation: HysteriaObfuscation,
}

#[derive(Clone)]
pub enum HysteriaObfuscation {
    Disabled,
    Salamander { password: String },
    Gecko { password: String },
}

#[derive(Clone)]
pub struct OpenVpnConfig {
    pub transport_id: String,
    pub manager_url: String,
    pub manager_token: String,
    pub auth_token: String,
    pub public_host: String,
    pub public_port: u16,
    pub sni: String,
    pub ca_certificate: String,
    pub stunnel_ca_certificate: String,
    pub tls_crypt_key: String,
}

pub const SHADOWSOCKS_2022_METHOD: &str = "2022-blake3-aes-256-gcm";

#[derive(Clone)]
pub struct ShadowsocksConfig {
    pub transport_id: String,
    pub manager_address: String,
    pub public_host: String,
    pub port_start: u16,
    pub port_end: u16,
    pub credential_key: String,
}

#[derive(Clone)]
pub struct WireGuardConfig {
    pub transport_id: String,
    pub manager_url: String,
    pub manager_token: String,
    pub credential_key: String,
    pub server_public_key: String,
    pub public_host: String,
    pub public_port: u16,
    pub sni: String,
    pub path_prefix: String,
}

#[derive(Clone)]
pub struct XrayConfig {
    pub runtime_config_path: PathBuf,
    pub binary_path: PathBuf,
    pub api_server: String,
    pub api_listen: String,
    pub api_port: u16,
    pub credential_key: String,
    pub reality: Option<XrayRealityConfig>,
    pub xhttp: Option<XrayXhttpConfig>,
}

#[derive(Clone)]
pub struct XrayRealityConfig {
    pub transport_id: String,
    pub inbound_tag: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub public_host: String,
    pub public_port: u16,
    pub sni: String,
    pub server_names: Vec<String>,
    pub reality_target: String,
    pub private_key: String,
    pub public_key: String,
    pub short_id: String,
    pub fingerprint: String,
}

#[derive(Clone)]
pub struct XrayXhttpConfig {
    pub transport_id: String,
    pub inbound_tag: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub public_host: String,
    pub public_port: u16,
    pub sni: String,
    pub path: String,
    pub fingerprint: String,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let bind_address = value("KET_BIND", "0.0.0.0:8787")
            .parse()
            .context("KET_BIND must be a valid socket address")?;
        let admin_token = env::var("KET_ADMIN_TOKEN")
            .context("KET_ADMIN_TOKEN is required and must contain at least 32 characters")?;
        if admin_token.len() < 32 {
            bail!("KET_ADMIN_TOKEN must contain at least 32 characters");
        }

        let public_url = normalize_public_url(value("KET_PUBLIC_URL", "http://127.0.0.1:8787"))?;

        let max_sessions = parse_value("KET_MAX_SESSIONS", 1000_u32)?;
        if max_sessions == 0 {
            bail!("KET_MAX_SESSIONS must be greater than zero");
        }
        let session_ttl_seconds = parse_value("KET_SESSION_TTL_SECONDS", 1800_u64)?;
        if !(60..=86_400).contains(&session_ttl_seconds) {
            bail!("KET_SESSION_TTL_SECONDS must be between 60 and 86400");
        }

        let mut transports = match env::var("KET_TRANSPORTS_JSON") {
            Ok(json) => serde_json::from_str(&json).context("KET_TRANSPORTS_JSON is invalid")?,
            Err(_) => Vec::new(),
        };
        let hysteria = HysteriaConfig::from_env()?;
        if let Some(config) = &hysteria {
            transports.push(config.transport_profile());
        }
        let openvpn = OpenVpnConfig::from_env(max_sessions)?;
        if let Some(config) = &openvpn {
            transports.push(config.transport_profile());
        }
        let xray = XrayConfig::from_env()?;
        if let Some(config) = &xray {
            if let Some(reality) = &config.reality {
                transports.push(reality.transport_profile());
            }
            if let Some(xhttp) = &config.xhttp {
                transports.push(xhttp.transport_profile());
            }
        }
        let shadowsocks = ShadowsocksConfig::from_env(max_sessions)?;
        if let Some(config) = &shadowsocks {
            transports.push(config.transport_profile());
        }
        let wireguard = WireGuardConfig::from_env(max_sessions)?;
        if let Some(config) = &wireguard {
            transports.push(config.transport_profile());
        }
        validate_transports(&transports)?;

        let node_id = value("KET_NODE_ID", "ket-node-1");
        let node_name = value("KET_NODE_NAME", "Ket node");
        let country_code = value("KET_COUNTRY_CODE", "ZZ");
        let country_name = value("KET_COUNTRY_NAME", "Unknown");
        let city = env::var("KET_CITY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let latitude = parse_value("KET_LATITUDE", 0.0_f64)?;
        let longitude = parse_value("KET_LONGITUDE", 0.0_f64)?;
        let location = NodeLocation {
            country_code,
            country_name,
            city,
            latitude,
            longitude,
        };
        validate_node_metadata(&node_id, &node_name, &location)?;

        Ok(Self {
            bind_address,
            state_path: value("KET_STATE_PATH", "/var/lib/ket/state.json").into(),
            admin_token,
            public_url,
            node_id,
            node_name,
            location,
            max_sessions,
            session_ttl: Duration::from_secs(session_ttl_seconds),
            transports,
            hysteria,
            openvpn,
            shadowsocks,
            wireguard,
            xray,
        })
    }
}

impl OpenVpnConfig {
    fn from_env(max_sessions: u32) -> Result<Option<Self>> {
        if !parse_value("KET_OPENVPN_ENABLED", false)? {
            return Ok(None);
        }
        if max_sessions > 65_533 {
            bail!("KET_MAX_SESSIONS cannot exceed 65533 when OpenVPN is enabled");
        }

        let manager_url = required("KET_OPENVPN_MANAGER_URL")?;
        validate_http_manager_url(&manager_url, "KET_OPENVPN_MANAGER_URL")?;
        let manager_token = required("KET_OPENVPN_MANAGER_TOKEN")?;
        if manager_token.len() < 32 {
            bail!("KET_OPENVPN_MANAGER_TOKEN must contain at least 32 characters");
        }
        let auth_token = required("KET_OPENVPN_AUTH_TOKEN")?;
        if auth_token.len() < 32 {
            bail!("KET_OPENVPN_AUTH_TOKEN must contain at least 32 characters");
        }
        if auth_token == manager_token {
            bail!("KET_OPENVPN_AUTH_TOKEN and KET_OPENVPN_MANAGER_TOKEN must be independent");
        }

        let ca_certificate = read_openvpn_material(
            "KET_OPENVPN_CA_CERT_PATH",
            "-----BEGIN CERTIFICATE-----",
            "-----END CERTIFICATE-----",
        )?;
        let stunnel_ca_certificate = read_openvpn_material(
            "KET_OPENVPN_STUNNEL_CA_CERT_PATH",
            "-----BEGIN CERTIFICATE-----",
            "-----END CERTIFICATE-----",
        )?;
        let tls_crypt_key = read_openvpn_material(
            "KET_OPENVPN_TLS_CRYPT_KEY_PATH",
            "-----BEGIN OpenVPN Static key V1-----",
            "-----END OpenVPN Static key V1-----",
        )?;

        Ok(Some(Self {
            transport_id: value("KET_OPENVPN_TRANSPORT_ID", "openvpn-stunnel-primary"),
            manager_url: manager_url.trim_end_matches('/').to_owned(),
            manager_token,
            auth_token,
            public_host: required_hostname("KET_OPENVPN_PUBLIC_HOST")?,
            public_port: nonzero_port("KET_OPENVPN_PUBLIC_PORT", 443)?,
            sni: required_dns_name("KET_OPENVPN_SNI")?,
            ca_certificate,
            stunnel_ca_certificate,
            tls_crypt_key,
        }))
    }

    fn transport_profile(&self) -> TransportProfile {
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "OpenVPN TLS".to_owned(),
            protocol: TransportProtocol::OpenVpnStunnel,
            endpoint: self.public_host.clone(),
            port: self.public_port,
            network: Network::Tcp,
            priority: 8,
            tls_server_name: Some(self.sni.clone()),
            options: BTreeMap::from([
                ("auth_mode".to_owned(), "session_token".to_owned()),
                ("cipher".to_owned(), "aes_256_gcm".to_owned()),
                ("remote_cert_tls".to_owned(), "server".to_owned()),
                ("tls_crypt".to_owned(), "v1".to_owned()),
                ("tls_minimum".to_owned(), "1.2".to_owned()),
                ("transport".to_owned(), "stunnel_tls".to_owned()),
            ]),
        }
    }
}

impl WireGuardConfig {
    fn from_env(max_sessions: u32) -> Result<Option<Self>> {
        if !parse_value("KET_WIREGUARD_ENABLED", false)? {
            return Ok(None);
        }
        if max_sessions > 65_533 {
            bail!("KET_MAX_SESSIONS cannot exceed 65533 when WireGuard TLS is enabled");
        }

        let manager_url = required("KET_WIREGUARD_MANAGER_URL")?;
        let parsed = Url::parse(&manager_url)
            .context("KET_WIREGUARD_MANAGER_URL must be an absolute HTTP URL")?;
        if parsed.scheme() != "http"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            bail!(
                "KET_WIREGUARD_MANAGER_URL must be an HTTP URL without credentials, a query, or a fragment"
            );
        }

        let manager_token = required("KET_WIREGUARD_MANAGER_TOKEN")?;
        if manager_token.len() < 32 {
            bail!("KET_WIREGUARD_MANAGER_TOKEN must contain at least 32 characters");
        }
        let credential_key = required("KET_WIREGUARD_CREDENTIAL_KEY")?;
        if credential_key.len() < 32 {
            bail!("KET_WIREGUARD_CREDENTIAL_KEY must contain at least 32 characters");
        }
        let server_public_key = required_wireguard_key("KET_WIREGUARD_SERVER_PUBLIC_KEY")?;
        let path_prefix = required("KET_WIREGUARD_WS_PATH_PREFIX")?;
        validate_wireguard_path_prefix(&path_prefix)?;

        Ok(Some(Self {
            transport_id: value("KET_WIREGUARD_TRANSPORT_ID", "wireguard-tls-primary"),
            manager_url: manager_url.trim_end_matches('/').to_owned(),
            manager_token,
            credential_key,
            server_public_key,
            public_host: required_hostname("KET_WIREGUARD_PUBLIC_HOST")?,
            public_port: nonzero_port("KET_WIREGUARD_PUBLIC_PORT", 443)?,
            sni: required_dns_name("KET_WIREGUARD_SNI")?,
            path_prefix,
        }))
    }

    pub(crate) fn client_address(&self, resource_slot: u32) -> Option<String> {
        let host = resource_slot.checked_add(2)?;
        (host <= 65_534).then(|| format!("10.66.{}.{}", host >> 8, host & 0xff))
    }

    fn transport_profile(&self) -> TransportProfile {
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "WireGuard TLS".to_owned(),
            protocol: TransportProtocol::WireGuard,
            endpoint: self.public_host.clone(),
            port: self.public_port,
            network: Network::Tcp,
            priority: 2,
            tls_server_name: Some(self.sni.clone()),
            options: BTreeMap::from([
                ("address_allocation".to_owned(), "lease_slot".to_owned()),
                ("allowed_ips".to_owned(), "0.0.0.0/0".to_owned()),
                ("keepalive_seconds".to_owned(), "25".to_owned()),
                ("mtu".to_owned(), "1280".to_owned()),
                ("path_prefix".to_owned(), self.path_prefix.clone()),
                (
                    "remote_address".to_owned(),
                    "wireguard-agent:51820".to_owned(),
                ),
                ("transport".to_owned(), "websocket_tls".to_owned()),
            ]),
        }
    }
}

impl ShadowsocksConfig {
    fn from_env(max_sessions: u32) -> Result<Option<Self>> {
        if !parse_value("KET_SHADOWSOCKS_ENABLED", false)? {
            return Ok(None);
        }

        let manager_address = value("KET_SHADOWSOCKS_MANAGER_ADDRESS", "shadowsocks:6100");
        validate_authority(&manager_address, "KET_SHADOWSOCKS_MANAGER_ADDRESS")?;
        let public_host = required_hostname("KET_SHADOWSOCKS_PUBLIC_HOST")?;
        if max_sessions > MAX_SHADOWSOCKS_MANAGER_SERVERS {
            bail!(
                "KET_MAX_SESSIONS cannot exceed {MAX_SHADOWSOCKS_MANAGER_SERVERS} when Shadowsocks is enabled"
            );
        }
        let port_start = nonzero_port("KET_SHADOWSOCKS_PORT_START", 20_000)?;
        let port_end = nonzero_port("KET_SHADOWSOCKS_PORT_END", 20_999)?;
        if port_end < port_start {
            bail!("KET_SHADOWSOCKS_PORT_END must not be lower than KET_SHADOWSOCKS_PORT_START");
        }
        let pool_size = u32::from(port_end) - u32::from(port_start) + 1;
        if pool_size < max_sessions {
            bail!(
                "the Shadowsocks port range must contain at least KET_MAX_SESSIONS ({max_sessions}) ports"
            );
        }
        let credential_key = required("KET_SHADOWSOCKS_CREDENTIAL_KEY")?;
        if credential_key.len() < 32 {
            bail!("KET_SHADOWSOCKS_CREDENTIAL_KEY must contain at least 32 characters");
        }

        Ok(Some(Self {
            transport_id: value("KET_SHADOWSOCKS_TRANSPORT_ID", "shadowsocks-2022-primary"),
            manager_address,
            public_host,
            port_start,
            port_end,
            credential_key,
        }))
    }

    pub(crate) fn session_port(&self, resource_slot: u32) -> Option<u16> {
        let port = u32::from(self.port_start).checked_add(resource_slot)?;
        (port <= u32::from(self.port_end)).then_some(port as u16)
    }

    fn transport_profile(&self) -> TransportProfile {
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "Shadowsocks 2022".to_owned(),
            protocol: TransportProtocol::Shadowsocks2022,
            endpoint: self.public_host.clone(),
            port: self.port_start,
            network: Network::TcpAndUdp,
            priority: 15,
            tls_server_name: None,
            options: BTreeMap::from([
                ("method".to_owned(), SHADOWSOCKS_2022_METHOD.to_owned()),
                ("mode".to_owned(), "tcp_and_udp".to_owned()),
                ("port_allocation".to_owned(), "lease_slot".to_owned()),
            ]),
        }
    }
}

impl HysteriaConfig {
    fn from_env() -> Result<Option<Self>> {
        if !parse_value("KET_HYSTERIA_ENABLED", false)? {
            return Ok(None);
        }

        let public_host = required_hostname("KET_HYSTERIA_PUBLIC_HOST")?;
        let public_port = parse_value("KET_HYSTERIA_PUBLIC_PORT", 443_u16)?;
        if public_port == 0 {
            bail!("KET_HYSTERIA_PUBLIC_PORT must be greater than zero");
        }
        let stats_secret = required("KET_HYSTERIA_STATS_SECRET")?;
        if stats_secret.len() < 32 {
            bail!("KET_HYSTERIA_STATS_SECRET must contain at least 32 characters");
        }
        let masquerade_url = required("KET_HYSTERIA_MASQUERADE_URL")?;
        if !masquerade_url.starts_with("https://") {
            bail!("KET_HYSTERIA_MASQUERADE_URL must use https://");
        }

        let obfuscation = match value("KET_HYSTERIA_OBFS", "none").as_str() {
            "none" => HysteriaObfuscation::Disabled,
            "salamander" => HysteriaObfuscation::Salamander {
                password: obfuscation_password()?,
            },
            "gecko" => HysteriaObfuscation::Gecko {
                password: obfuscation_password()?,
            },
            _ => bail!("KET_HYSTERIA_OBFS must be none, salamander, or gecko"),
        };

        Ok(Some(Self {
            transport_id: value("KET_HYSTERIA_TRANSPORT_ID", "hy2-primary"),
            runtime_config_path: value(
                "KET_HYSTERIA_CONFIG_PATH",
                "/var/lib/ket-dataplane/hysteria.json",
            )
            .into(),
            listen: value("KET_HYSTERIA_LISTEN", ":8443"),
            sni: value("KET_HYSTERIA_SNI", &public_host),
            public_host,
            public_port,
            tls_cert_path: value(
                "KET_HYSTERIA_TLS_CERT_PATH",
                "/etc/hysteria/tls/fullchain.pem",
            ),
            tls_key_path: value("KET_HYSTERIA_TLS_KEY_PATH", "/etc/hysteria/tls/privkey.pem"),
            auth_url: value(
                "KET_HYSTERIA_AUTH_URL",
                "http://control-plane:8787/internal/v1/hysteria2/auth",
            ),
            stats_url: value("KET_HYSTERIA_STATS_URL", "http://hysteria2:9999"),
            stats_secret,
            masquerade_url,
            obfuscation,
        }))
    }

    fn transport_profile(&self) -> TransportProfile {
        let mut options = BTreeMap::new();
        match &self.obfuscation {
            HysteriaObfuscation::Disabled => {}
            HysteriaObfuscation::Salamander { .. } => {
                options.insert("obfs".to_owned(), "salamander".to_owned());
            }
            HysteriaObfuscation::Gecko { .. } => {
                options.insert("obfs".to_owned(), "gecko".to_owned());
                options.insert("gecko_min_packet_size".to_owned(), "512".to_owned());
                options.insert("gecko_max_packet_size".to_owned(), "1200".to_owned());
            }
        }
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "Hysteria 2".to_owned(),
            protocol: TransportProtocol::Hysteria2,
            endpoint: self.public_host.clone(),
            port: self.public_port,
            network: Network::Udp,
            priority: 10,
            tls_server_name: Some(self.sni.clone()),
            options,
        }
    }
}

impl XrayConfig {
    fn from_env() -> Result<Option<Self>> {
        let reality_enabled = parse_value("KET_XRAY_ENABLED", false)?;
        let xhttp_enabled = parse_value("KET_XHTTP_ENABLED", false)?;
        if !reality_enabled && !xhttp_enabled {
            return Ok(None);
        }

        let api_port = nonzero_port("KET_XRAY_API_PORT", 10085)?;
        let credential_key = required("KET_XRAY_CREDENTIAL_KEY")?;
        if credential_key.len() < 32 {
            bail!("KET_XRAY_CREDENTIAL_KEY must contain at least 32 characters");
        }
        let api_server = required("KET_XRAY_API_SERVER")?;
        validate_authority(&api_server, "KET_XRAY_API_SERVER")?;
        let api_listen = value("KET_XRAY_API_LISTEN", "0.0.0.0");
        validate_listen_host(&api_listen, "KET_XRAY_API_LISTEN")?;
        let reality = reality_enabled
            .then(XrayRealityConfig::from_env)
            .transpose()?;
        let xhttp = xhttp_enabled.then(XrayXhttpConfig::from_env).transpose()?;
        if reality.as_ref().is_some_and(|reality| {
            xhttp
                .as_ref()
                .is_some_and(|xhttp| reality.inbound_tag == xhttp.inbound_tag)
        }) {
            bail!("KET_XRAY_INBOUND_TAG and KET_XHTTP_INBOUND_TAG must be different");
        }
        if reality.as_ref().is_some_and(|reality| {
            listeners_conflict(
                &api_listen,
                api_port,
                &reality.listen_host,
                reality.listen_port,
            )
        }) {
            bail!("KET_XRAY_API and KET_XRAY inbound listeners must not overlap");
        }
        if xhttp.as_ref().is_some_and(|xhttp| {
            listeners_conflict(&api_listen, api_port, &xhttp.listen_host, xhttp.listen_port)
        }) {
            bail!("KET_XRAY_API and KET_XHTTP inbound listeners must not overlap");
        }
        if reality.as_ref().is_some_and(|reality| {
            xhttp.as_ref().is_some_and(|xhttp| {
                listeners_conflict(
                    &reality.listen_host,
                    reality.listen_port,
                    &xhttp.listen_host,
                    xhttp.listen_port,
                )
            })
        }) {
            bail!("KET_XRAY and KET_XHTTP inbound listeners must not overlap");
        }

        Ok(Some(Self {
            runtime_config_path: value("KET_XRAY_CONFIG_PATH", "/var/lib/ket-dataplane/xray.json")
                .into(),
            binary_path: value("KET_XRAY_BINARY", "/usr/local/bin/xray").into(),
            api_server,
            api_listen,
            api_port,
            credential_key,
            reality,
            xhttp,
        }))
    }
}

impl XrayRealityConfig {
    fn from_env() -> Result<Self> {
        let public_host = required_hostname("KET_XRAY_PUBLIC_HOST")?;
        let sni = required_dns_name("KET_XRAY_SNI")?;
        let server_names = required("KET_XRAY_SERVER_NAMES")?
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if server_names.is_empty() {
            bail!("KET_XRAY_SERVER_NAMES must contain at least one hostname");
        }
        for name in &server_names {
            validate_dns_name(name, "KET_XRAY_SERVER_NAMES")?;
        }
        if !server_names.iter().any(|name| name == &sni) {
            bail!("KET_XRAY_SNI must be listed in KET_XRAY_SERVER_NAMES");
        }
        let reality_target = required("KET_XRAY_REALITY_TARGET")?;
        validate_target(&reality_target)?;
        let private_key = required_reality_key("KET_XRAY_PRIVATE_KEY")?;
        let public_key = required_reality_key("KET_XRAY_PUBLIC_KEY")?;
        if private_key == public_key {
            bail!("KET_XRAY_PRIVATE_KEY and KET_XRAY_PUBLIC_KEY must be different");
        }
        let short_id = required("KET_XRAY_SHORT_ID")?;
        if short_id.len() != 16 || !short_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("KET_XRAY_SHORT_ID must contain exactly 16 hexadecimal characters");
        }
        let listen_host = value("KET_XRAY_LISTEN_HOST", "0.0.0.0");
        validate_listen_host(&listen_host, "KET_XRAY_LISTEN_HOST")?;

        Ok(Self {
            transport_id: value("KET_XRAY_TRANSPORT_ID", "vless-reality-primary"),
            inbound_tag: bounded_tag("KET_XRAY_INBOUND_TAG", "vless-reality")?,
            listen_host,
            listen_port: nonzero_port("KET_XRAY_LISTEN_PORT", 8444)?,
            public_host,
            public_port: nonzero_port("KET_XRAY_PUBLIC_PORT", 443)?,
            sni,
            server_names,
            reality_target,
            private_key,
            public_key,
            short_id: short_id.to_lowercase(),
            fingerprint: fingerprint("KET_XRAY_FINGERPRINT")?,
        })
    }

    fn transport_profile(&self) -> TransportProfile {
        let options = BTreeMap::from([
            ("encryption".to_owned(), "none".to_owned()),
            ("fingerprint".to_owned(), self.fingerprint.clone()),
            ("flow".to_owned(), "xtls-rprx-vision".to_owned()),
            ("transport".to_owned(), "raw".to_owned()),
        ]);
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "VLESS + REALITY".to_owned(),
            protocol: TransportProtocol::VlessXtlsReality,
            endpoint: self.public_host.clone(),
            port: self.public_port,
            network: Network::Tcp,
            priority: 5,
            tls_server_name: Some(self.sni.clone()),
            options,
        }
    }
}

impl XrayXhttpConfig {
    fn from_env() -> Result<Self> {
        let listen_host = value("KET_XHTTP_LISTEN_HOST", "0.0.0.0");
        validate_listen_host(&listen_host, "KET_XHTTP_LISTEN_HOST")?;
        let path = required("KET_XHTTP_PATH")?;
        validate_xhttp_path(&path)?;
        Ok(Self {
            transport_id: value("KET_XHTTP_TRANSPORT_ID", "https-stealth-primary"),
            inbound_tag: bounded_tag("KET_XHTTP_INBOUND_TAG", "vless-xhttp")?,
            listen_host,
            listen_port: nonzero_port("KET_XHTTP_LISTEN_PORT", 8445)?,
            public_host: required_hostname("KET_XHTTP_PUBLIC_HOST")?,
            public_port: nonzero_port("KET_XHTTP_PUBLIC_PORT", 443)?,
            sni: required_dns_name("KET_XHTTP_SNI")?,
            path,
            fingerprint: fingerprint("KET_XHTTP_FINGERPRINT")?,
        })
    }

    fn transport_profile(&self) -> TransportProfile {
        TransportProfile {
            id: self.transport_id.clone(),
            display_name: "HTTPS Stealth".to_owned(),
            protocol: TransportProtocol::Stealth,
            endpoint: self.public_host.clone(),
            port: self.public_port,
            network: Network::Tcp,
            priority: 1,
            tls_server_name: Some(self.sni.clone()),
            options: BTreeMap::from([
                ("encryption".to_owned(), "none".to_owned()),
                ("fingerprint".to_owned(), self.fingerprint.clone()),
                ("mode".to_owned(), "packet-up".to_owned()),
                ("path".to_owned(), self.path.clone()),
                ("security".to_owned(), "tls".to_owned()),
                ("transport".to_owned(), "xhttp".to_owned()),
            ]),
        }
    }
}

fn bounded_tag(name: &str, default: &str) -> Result<String> {
    let tag = value(name, default);
    if tag.trim().is_empty() || tag.len() > 64 || tag.chars().any(char::is_control) {
        bail!("{name} must be 1-64 printable characters");
    }
    Ok(tag)
}

fn fingerprint(name: &str) -> Result<String> {
    let fingerprint = value(name, "chrome");
    if !matches!(
        fingerprint.as_str(),
        "chrome" | "firefox" | "safari" | "ios" | "android" | "edge" | "random"
    ) {
        bail!("{name} is unsupported");
    }
    Ok(fingerprint)
}

fn validate_xhttp_path(path: &str) -> Result<()> {
    if !(16..=128).contains(&path.len())
        || !path.starts_with('/')
        || path.ends_with('/')
        || path.contains("//")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
    {
        bail!(
            "KET_XHTTP_PATH must be a 16-128 character absolute path using letters, numbers, /, -, or _"
        );
    }
    Ok(())
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn required(name: &str) -> Result<String> {
    let result = env::var(name).with_context(|| format!("{name} is required"))?;
    if result.trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(result)
}

fn required_hostname(name: &str) -> Result<String> {
    let hostname = required(name)?;
    validate_hostname(&hostname, name)?;
    Ok(hostname)
}

fn required_dns_name(name: &str) -> Result<String> {
    let hostname = required(name)?;
    validate_dns_name(&hostname, name)?;
    Ok(hostname)
}

fn validate_hostname(hostname: &str, name: &str) -> Result<()> {
    if hostname.is_empty()
        || hostname.len() > MAX_ENDPOINT_CHARS
        || hostname != hostname.trim()
        || hostname.contains("://")
        || hostname.contains('/')
        || hostname.contains('\\')
        || hostname.contains('?')
        || hostname.contains('#')
        || hostname.chars().any(char::is_whitespace)
    {
        bail!("{name} must be a hostname or IP address without a scheme or path");
    }
    if hostname.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if hostname.starts_with('.')
        || hostname.ends_with('.')
        || !hostname.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        bail!("{name} must be a valid hostname or IP address");
    }
    Ok(())
}

fn validate_dns_name(hostname: &str, name: &str) -> Result<()> {
    validate_hostname(hostname, name)?;
    if hostname.parse::<std::net::IpAddr>().is_ok() {
        bail!("{name} must be a DNS hostname, not an IP address");
    }
    Ok(())
}

fn validate_listen_host(host: &str, name: &str) -> Result<()> {
    if host.parse::<std::net::IpAddr>().is_err() {
        bail!("{name} must be an IPv4 or IPv6 address");
    }
    Ok(())
}

fn listeners_conflict(
    first_host: &str,
    first_port: u16,
    second_host: &str,
    second_port: u16,
) -> bool {
    if first_port != second_port {
        return false;
    }
    let first = first_host
        .parse::<std::net::IpAddr>()
        .expect("listen hosts are validated before conflict checks");
    let second = second_host
        .parse::<std::net::IpAddr>()
        .expect("listen hosts are validated before conflict checks");
    match (first, second) {
        (std::net::IpAddr::V4(first), std::net::IpAddr::V4(second)) => {
            first == second || first.is_unspecified() || second.is_unspecified()
        }
        (std::net::IpAddr::V6(first), std::net::IpAddr::V6(second)) => {
            first == second || first.is_unspecified() || second.is_unspecified()
        }
        _ => false,
    }
}

fn validate_authority(authority: &str, name: &str) -> Result<()> {
    let Some((host, port)) = authority.rsplit_once(':') else {
        bail!("{name} must use host:port format");
    };
    validate_hostname(host, name)?;
    if port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
        bail!("{name} must contain a non-zero port");
    }
    Ok(())
}

fn validate_target(target: &str) -> Result<()> {
    validate_authority(target, "KET_XRAY_REALITY_TARGET")
}

fn required_reality_key(name: &str) -> Result<String> {
    let key = required(name)?;
    if key.len() != 43
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        bail!("{name} must be a 43-character base64url X25519 key");
    }
    Ok(key)
}

fn required_wireguard_key(name: &str) -> Result<String> {
    let key = required(name)?;
    if STANDARD
        .decode(&key)
        .ok()
        .is_none_or(|decoded| decoded.len() != 32)
    {
        bail!("{name} must be a standard-base64 WireGuard key containing exactly 32 bytes");
    }
    Ok(key)
}

fn validate_wireguard_path_prefix(prefix: &str) -> Result<()> {
    if !(16..=96).contains(&prefix.len())
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!(
            "KET_WIREGUARD_WS_PATH_PREFIX must contain 16-96 letters, numbers, hyphens, or underscores"
        );
    }
    Ok(())
}

fn validate_http_manager_url(value: &str, name: &str) -> Result<()> {
    let parsed = Url::parse(value).with_context(|| format!("{name} must be an absolute URL"))?;
    if parsed.scheme() != "http"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("{name} must be an HTTP URL without credentials, a query, or a fragment");
    }
    Ok(())
}

fn read_openvpn_material(name: &str, begin: &str, end: &str) -> Result<String> {
    let path = PathBuf::from(required(name)?);
    let metadata = fs::metadata(&path)
        .with_context(|| format!("{name} does not reference a readable regular file"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OPENVPN_MATERIAL_BYTES {
        bail!("{name} must reference a non-empty regular file no larger than 3 KiB");
    }
    let material = fs::read_to_string(&path)
        .with_context(|| format!("{name} must reference UTF-8 PEM material"))?;
    let material = material.trim().to_owned();
    if !material.starts_with(begin)
        || !material.ends_with(end)
        || material.contains('\0')
        || material.lines().any(|line| line.len() > 256)
    {
        bail!("{name} contains invalid OpenVPN key material");
    }
    Ok(format!("{material}\n"))
}

fn nonzero_port(name: &str, default: u16) -> Result<u16> {
    let port = parse_value(name, default)?;
    if port == 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(port)
}

fn obfuscation_password() -> Result<String> {
    let password = required("KET_HYSTERIA_OBFS_PASSWORD")?;
    if password.len() < 32 {
        bail!("KET_HYSTERIA_OBFS_PASSWORD must contain at least 32 characters");
    }
    Ok(password)
}

fn parse_value<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr + Copy,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("{name} is invalid")),
        Err(_) => Ok(default),
    }
}

fn validate_transports(transports: &[TransportProfile]) -> Result<()> {
    if transports.len() > MAX_TRANSPORTS {
        bail!("at most {MAX_TRANSPORTS} transports may be advertised");
    }
    let mut ids = HashSet::new();
    for transport in transports {
        validate_identifier(&transport.id, "transport ID", MAX_TRANSPORT_ID_CHARS)?;
        validate_text(
            &transport.display_name,
            "transport display name",
            MAX_NODE_TEXT_CHARS,
        )?;
        if validate_hostname(&transport.endpoint, "transport endpoint").is_err()
            || transport.port == 0
        {
            bail!("transport {} has an invalid endpoint", transport.id);
        }
        if let Some(server_name) = &transport.tls_server_name {
            validate_hostname(server_name, "transport TLS server name")?;
        }
        if transport.options.len() > MAX_OPTION_ENTRIES {
            bail!("transport {} contains too many options", transport.id);
        }
        for (key, value) in &transport.options {
            validate_option_key(key, "transport option")?;
            if value.chars().count() > MAX_OPTION_VALUE_CHARS || value.chars().any(char::is_control)
            {
                bail!(
                    "transport {} contains an invalid option value",
                    transport.id
                );
            }
        }
        if !ids.insert(&transport.id) {
            bail!("transport ids must be unique: {}", transport.id);
        }
    }
    Ok(())
}

fn normalize_public_url(value: String) -> Result<String> {
    validate_text(&value, "KET_PUBLIC_URL", MAX_PUBLIC_URL_CHARS)?;
    let parsed = Url::parse(&value).context("KET_PUBLIC_URL must be an absolute URL")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("KET_PUBLIC_URL must be an HTTP(S) URL without credentials, a query, or a fragment");
    }
    if parsed.scheme() == "http" && !is_loopback_url(&parsed) {
        bail!("KET_PUBLIC_URL must use HTTPS unless it targets loopback development");
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn validate_node_metadata(node_id: &str, node_name: &str, location: &NodeLocation) -> Result<()> {
    validate_identifier(node_id, "KET_NODE_ID", MAX_NODE_TEXT_CHARS)?;
    validate_text(node_name, "KET_NODE_NAME", MAX_NODE_TEXT_CHARS)?;
    validate_text(
        &location.country_name,
        "KET_COUNTRY_NAME",
        MAX_NODE_TEXT_CHARS,
    )?;
    if let Some(city) = &location.city {
        validate_text(city, "KET_CITY", MAX_NODE_TEXT_CHARS)?;
    }
    validate_location(
        &location.country_code,
        location.latitude,
        location.longitude,
    )
}

fn validate_identifier(value: &str, label: &str, maximum_chars: usize) -> Result<()> {
    if value.is_empty()
        || value.chars().count() > maximum_chars
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("{label} has an invalid identifier shape");
    }
    Ok(())
}

fn validate_text(value: &str, label: &str, maximum_chars: usize) -> Result<()> {
    if value.is_empty()
        || value != value.trim()
        || value.chars().count() > maximum_chars
        || value.chars().any(char::is_control)
    {
        bail!("{label} must contain 1-{maximum_chars} trimmed printable characters");
    }
    Ok(())
}

fn validate_option_key(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_OPTION_KEY_CHARS
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("{label} keys must use 1-{MAX_OPTION_KEY_CHARS} lowercase ASCII characters");
    }
    Ok(())
}

fn validate_location(country_code: &str, latitude: f64, longitude: f64) -> Result<()> {
    if country_code.len() != 2 || !country_code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        bail!("KET_COUNTRY_CODE must be a two-letter uppercase ISO code");
    }
    if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        bail!("KET_LATITUDE must be between -90 and 90");
    }
    if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
        bail!("KET_LONGITUDE must be between -180 and 180");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ket_core::{Network, NodeLocation, TransportProfile, TransportProtocol};

    use super::{
        ShadowsocksConfig, listeners_conflict, normalize_public_url, required_reality_key,
        validate_authority, validate_dns_name, validate_hostname, validate_location,
        validate_node_metadata, validate_target, validate_transports, validate_xhttp_path,
    };

    #[test]
    fn public_url_validation_rejects_ambiguous_control_endpoints() {
        assert_eq!(
            normalize_public_url("https://ket.example.test/control/".to_owned()).unwrap(),
            "https://ket.example.test/control"
        );
        assert!(normalize_public_url("http://127.0.0.1:8787".to_owned()).is_ok());
        assert!(normalize_public_url("http://[::1]:8787".to_owned()).is_ok());
        assert!(normalize_public_url("http://ket.example.test".to_owned()).is_err());
        assert!(normalize_public_url("ftp://ket.example.test".to_owned()).is_err());
        assert!(normalize_public_url("https://user@ket.example.test".to_owned()).is_err());
        assert!(normalize_public_url("https://ket.example.test?token=nope".to_owned()).is_err());
        assert!(normalize_public_url("https://ket.example.test#fragment".to_owned()).is_err());
        assert!(normalize_public_url(" https://ket.example.test".to_owned()).is_err());
    }

    #[test]
    fn node_metadata_validation_matches_client_bounds() {
        let location = NodeLocation {
            country_code: "DE".to_owned(),
            country_name: "Germany".to_owned(),
            city: Some("Frankfurt".to_owned()),
            latitude: 50.1109,
            longitude: 8.6821,
        };
        assert!(validate_node_metadata("de-fra-1", "Frankfurt", &location).is_ok());
        assert!(validate_node_metadata("../node", "Frankfurt", &location).is_err());
        assert!(validate_node_metadata("de-fra-1", " Frankfurt", &location).is_err());

        let mut malformed = location.clone();
        malformed.country_name = "x".repeat(129);
        assert!(validate_node_metadata("de-fra-1", "Frankfurt", &malformed).is_err());
        malformed = location.clone();
        malformed.city = Some("Frankfurt\nspoofed".to_owned());
        assert!(validate_node_metadata("de-fra-1", "Frankfurt", &malformed).is_err());
    }

    #[test]
    fn location_validation_rejects_invalid_map_coordinates() {
        assert!(validate_location("US", 37.7, -122.4).is_ok());
        assert!(validate_location("usa", 0.0, 0.0).is_err());
        assert!(validate_location("ZZ", 91.0, 0.0).is_err());
        assert!(validate_location("ZZ", 0.0, -181.0).is_err());
        assert!(validate_location("ZZ", f64::NAN, 0.0).is_err());
    }

    #[test]
    fn transport_validation_rejects_ambiguous_endpoints_and_duplicate_ids() {
        let profile = |id: &str, endpoint: &str| TransportProfile {
            id: id.to_owned(),
            display_name: "Test".to_owned(),
            protocol: TransportProtocol::Stealth,
            endpoint: endpoint.to_owned(),
            port: 443,
            network: Network::Tcp,
            priority: 1,
            tls_server_name: None,
            options: BTreeMap::new(),
        };
        assert!(validate_transports(&[profile("one", "vpn.example")]).is_ok());
        assert!(validate_transports(&[profile("one", "https://vpn.example")]).is_err());
        assert!(validate_transports(&[profile("one", "vpn.example/path")]).is_err());
        assert!(validate_transports(&[profile("../one", "vpn.example")]).is_err());
        assert!(validate_transports(&[profile("one", "vpn.example?target=other")]).is_err());
        assert!(
            validate_transports(&[
                profile("one", "vpn.example"),
                profile("one", "other.example")
            ])
            .is_err()
        );

        let excessive = (0..33)
            .map(|index| profile(&format!("transport-{index}"), "vpn.example"))
            .collect::<Vec<_>>();
        assert!(validate_transports(&excessive).is_err());

        let mut malformed_option = profile("one", "vpn.example");
        malformed_option
            .options
            .insert("UPPERCASE".to_owned(), "value".to_owned());
        assert!(validate_transports(&[malformed_option]).is_err());
    }

    #[test]
    fn xray_values_reject_ambiguous_hosts_targets_and_keys() {
        assert!(validate_hostname("vpn.example.test", "HOST").is_ok());
        assert!(validate_hostname("", "HOST").is_err());
        assert!(validate_hostname("https://vpn.example.test", "HOST").is_err());
        assert!(validate_hostname("bad_host.example", "HOST").is_err());
        assert!(validate_hostname("2001:db8::1", "HOST").is_ok());
        assert!(validate_dns_name("www.example.com", "SNI").is_ok());
        assert!(validate_dns_name("203.0.113.1", "SNI").is_err());
        assert!(validate_target("www.example.com:443").is_ok());
        assert!(validate_target("https://www.example.com:443").is_err());
        assert!(validate_authority("xray:10085", "API").is_ok());
        assert!(validate_authority("xray", "API").is_err());
        assert!(listeners_conflict("0.0.0.0", 8445, "127.0.0.1", 8445));
        assert!(!listeners_conflict("0.0.0.0", 8444, "0.0.0.0", 8445));
        assert!(!listeners_conflict("0.0.0.0", 8445, "::", 8445));
        assert!(validate_xhttp_path("/a1b2c3d4e5f6g7h8").is_ok());
        assert!(validate_xhttp_path("/short").is_err());
        assert!(validate_xhttp_path("/a1b2c3d4e5f6g7h8/").is_err());
        assert!(validate_xhttp_path("/a1b2c3d4//e5f6g7h8").is_err());

        let name = "KET_TEST_REALITY_KEY";
        // SAFETY: this test is the only reader/writer of this test-specific variable.
        unsafe {
            std::env::set_var(name, "GMUeujXct7_Ig4N9J5asVItA8mXOMXBXGzcdMowh5Ag");
        }
        assert!(required_reality_key(name).is_ok());
        // SAFETY: this test-specific variable is no longer needed.
        unsafe { std::env::remove_var(name) };
    }

    #[test]
    fn shadowsocks_session_ports_are_bounded_by_the_validated_pool() {
        let config = ShadowsocksConfig {
            transport_id: "shadowsocks-2022-primary".to_owned(),
            manager_address: "shadowsocks:6100".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            port_start: 20_000,
            port_end: 20_002,
            credential_key: "independent-test-key-at-least-32-characters".to_owned(),
        };

        assert_eq!(config.session_port(0), Some(20_000));
        assert_eq!(config.session_port(2), Some(20_002));
        assert_eq!(config.session_port(3), None);
        assert_eq!(config.session_port(u32::MAX), None);
    }
}
