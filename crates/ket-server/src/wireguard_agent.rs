use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, SocketAddr},
    process::{Output, Stdio},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::{io::AsyncWriteExt, net::TcpListener, process::Command};
use zeroize::{Zeroize, Zeroizing};

use crate::wireguard::{ManagedPeer, PeerStatus, ReconcilePeers, RemovePeers};

const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct AgentConfig {
    bind_address: SocketAddr,
    interface: String,
    egress_interface: String,
    listen_port: u16,
    private_key: String,
    manager_token: String,
    max_peers: usize,
}

impl AgentConfig {
    fn from_env() -> Result<Self> {
        let bind_address = value("KET_WIREGUARD_AGENT_BIND", "0.0.0.0:8788")
            .parse()
            .context("KET_WIREGUARD_AGENT_BIND must be a socket address")?;
        let interface = value("KET_WIREGUARD_INTERFACE", "ketwg0");
        let egress_interface = value("KET_WIREGUARD_EGRESS_INTERFACE", "eth0");
        validate_interface(&interface, "KET_WIREGUARD_INTERFACE")?;
        validate_interface(&egress_interface, "KET_WIREGUARD_EGRESS_INTERFACE")?;
        let listen_port = value("KET_WIREGUARD_LISTEN_PORT", "51820")
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .context("KET_WIREGUARD_LISTEN_PORT must be a non-zero port")?;
        let private_key = required("KET_WIREGUARD_SERVER_PRIVATE_KEY")?;
        validate_key(&private_key, "KET_WIREGUARD_SERVER_PRIVATE_KEY")?;
        let manager_token = required("KET_WIREGUARD_MANAGER_TOKEN")?;
        if manager_token.len() < 32 {
            bail!("KET_WIREGUARD_MANAGER_TOKEN must contain at least 32 characters");
        }
        let max_peers = value("KET_WIREGUARD_MAX_PEERS", "1000")
            .parse::<usize>()
            .ok()
            .filter(|count| (1..=65_533).contains(count))
            .context("KET_WIREGUARD_MAX_PEERS must be between 1 and 65533")?;
        Ok(Self {
            bind_address,
            interface,
            egress_interface,
            listen_port,
            private_key,
            manager_token,
            max_peers,
        })
    }
}

#[derive(Clone)]
struct AgentState(Arc<AgentConfig>);

#[derive(Serialize)]
struct AgentError {
    code: &'static str,
    message: &'static str,
}

type ApiError = (StatusCode, Json<AgentError>);

pub async fn run() -> Result<()> {
    let mut config = AgentConfig::from_env()?;
    initialize_interface(&config).await?;
    config.private_key.zeroize();
    let config = Arc::new(config);
    let listener = TcpListener::bind(config.bind_address)
        .await
        .with_context(|| format!("failed to bind WireGuard agent on {}", config.bind_address))?;
    let state = AgentState(Arc::clone(&config));
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/peers", get(list_peers).put(upsert_peer))
        .route("/v1/peers/reconcile", put(reconcile_peers))
        .route("/v1/peers/remove", post(remove_peers))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state);
    tracing::info!(address = %config.bind_address, interface = %config.interface, "WireGuard agent ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("WireGuard agent stopped unexpectedly")?;
    Ok(())
}

async fn authorize(
    State(state): State<AgentState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let candidate = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let expected = state.0.manager_token.as_bytes();
    let accepted = candidate.is_some_and(|candidate| {
        candidate.len() == expected.len() && bool::from(candidate.as_bytes().ct_eq(expected))
    });
    if accepted {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(AgentError {
                code: "unauthorized",
                message: "manager authentication failed",
            }),
        )
            .into_response()
    }
}

async fn health(State(state): State<AgentState>) -> Result<StatusCode, ApiError> {
    run_command("wg", &["show", &state.0.interface])
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(internal_error)
}

async fn list_peers(State(state): State<AgentState>) -> Result<Json<Vec<PeerStatus>>, ApiError> {
    peer_statuses(&state.0)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn upsert_peer(
    State(state): State<AgentState>,
    Json(peer): Json<ManagedPeer>,
) -> Result<StatusCode, ApiError> {
    validate_peer(&peer).map_err(invalid_peer)?;
    install_peer(&state.0, &peer)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reconcile_peers(
    State(state): State<AgentState>,
    Json(request): Json<ReconcilePeers>,
) -> Result<StatusCode, ApiError> {
    if request.peers.len() > state.0.max_peers {
        return Err(invalid_peer(anyhow::anyhow!("peer limit exceeded")));
    }
    let mut desired = BTreeSet::new();
    for peer in &request.peers {
        validate_peer(peer).map_err(invalid_peer)?;
        if !desired.insert(peer.address.clone()) {
            return Err(invalid_peer(anyhow::anyhow!("duplicate peer address")));
        }
    }
    for current in peer_statuses(&state.0).await.map_err(internal_error)? {
        if !desired.contains(&current.address) {
            remove_public_key(&state.0, &current.public_key)
                .await
                .map_err(internal_error)?;
        }
    }
    for peer in &request.peers {
        install_peer(&state.0, peer).await.map_err(internal_error)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_peers(
    State(state): State<AgentState>,
    Json(request): Json<RemovePeers>,
) -> Result<StatusCode, ApiError> {
    if request.addresses.len() > state.0.max_peers {
        return Err(invalid_peer(anyhow::anyhow!("peer limit exceeded")));
    }
    let addresses = request.addresses.into_iter().collect::<BTreeSet<_>>();
    if addresses
        .iter()
        .any(|address| valid_address(address).is_err())
    {
        return Err(invalid_peer(anyhow::anyhow!("invalid peer address")));
    }
    for current in peer_statuses(&state.0).await.map_err(internal_error)? {
        if addresses.contains(&current.address) {
            remove_public_key(&state.0, &current.public_key)
                .await
                .map_err(internal_error)?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn initialize_interface(config: &AgentConfig) -> Result<()> {
    if run_command("ip", &["link", "show", "dev", &config.interface])
        .await
        .is_err()
    {
        run_command(
            "ip",
            &["link", "add", "dev", &config.interface, "type", "wireguard"],
        )
        .await?;
    }
    run_secret_command(
        "wg",
        &[
            "set",
            &config.interface,
            "listen-port",
            &config.listen_port.to_string(),
            "private-key",
            "/dev/stdin",
        ],
        &config.private_key,
    )
    .await?;
    run_command(
        "ip",
        &[
            "address",
            "replace",
            "10.66.0.1/16",
            "dev",
            &config.interface,
        ],
    )
    .await?;
    run_command(
        "ip",
        &["link", "set", "dev", &config.interface, "mtu", "1280", "up"],
    )
    .await?;
    install_firewall(config).await
}

async fn install_firewall(config: &AgentConfig) -> Result<()> {
    for destination in [
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "224.0.0.0/4",
    ] {
        ensure_iptables(&[
            "FORWARD",
            "-i",
            &config.interface,
            "-d",
            destination,
            "-j",
            "REJECT",
        ])
        .await?;
    }
    ensure_iptables(&[
        "FORWARD",
        "-i",
        &config.interface,
        "-p",
        "tcp",
        "-m",
        "multiport",
        "--dports",
        "25,465,587",
        "-j",
        "REJECT",
    ])
    .await?;
    ensure_iptables(&["FORWARD", "-i", &config.interface, "-j", "ACCEPT"]).await?;
    ensure_iptables(&[
        "FORWARD",
        "-o",
        &config.interface,
        "-m",
        "conntrack",
        "--ctstate",
        "RELATED,ESTABLISHED",
        "-j",
        "ACCEPT",
    ])
    .await?;
    ensure_iptables_table(
        "nat",
        &[
            "POSTROUTING",
            "-s",
            "10.66.0.0/16",
            "-o",
            &config.egress_interface,
            "-j",
            "MASQUERADE",
        ],
    )
    .await
}

async fn ensure_iptables(rule: &[&str]) -> Result<()> {
    ensure_iptables_table("filter", rule).await
}

async fn ensure_iptables_table(table: &str, rule: &[&str]) -> Result<()> {
    let mut check = vec!["-t", table, "-C"];
    check.extend_from_slice(rule);
    if run_command("iptables", &check).await.is_ok() {
        return Ok(());
    }
    let mut add = vec!["-t", table, "-A"];
    add.extend_from_slice(rule);
    run_command("iptables", &add).await.map(|_| ())
}

async fn install_peer(config: &AgentConfig, peer: &ManagedPeer) -> Result<()> {
    let public_key = derive_public_key(peer.private_key.expose_secret()).await?;
    let allowed_ip = format!("{}/32", peer.address);
    run_secret_command(
        "wg",
        &[
            "set",
            &config.interface,
            "peer",
            &public_key,
            "preshared-key",
            "/dev/stdin",
            "allowed-ips",
            &allowed_ip,
        ],
        peer.preshared_key.expose_secret(),
    )
    .await
    .map(|_| ())
}

async fn remove_public_key(config: &AgentConfig, public_key: &str) -> Result<()> {
    validate_key(public_key, "peer public key")?;
    run_command(
        "wg",
        &["set", &config.interface, "peer", public_key, "remove"],
    )
    .await
    .map(|_| ())
}

async fn derive_public_key(private_key: &str) -> Result<String> {
    let output = run_secret_command("wg", &["pubkey"], private_key).await?;
    let public_key = String::from_utf8(output.stdout)
        .context("wg returned a non-UTF-8 public key")?
        .trim()
        .to_owned();
    validate_key(&public_key, "derived peer public key")?;
    Ok(public_key)
}

async fn peer_statuses(config: &AgentConfig) -> Result<Vec<PeerStatus>> {
    let output = run_command("wg", &["show", &config.interface, "dump"]).await?;
    let output = String::from_utf8(output.stdout).context("wg dump was not UTF-8")?;
    parse_peer_statuses(&output)
}

fn parse_peer_statuses(output: &str) -> Result<Vec<PeerStatus>> {
    let mut peers = Vec::new();
    for line in output.lines().skip(1).filter(|line| !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 8 {
            bail!("wg returned a malformed peer row");
        }
        validate_key(fields[0], "peer public key")?;
        let allowed = fields[3].split(',').collect::<Vec<_>>();
        if allowed.len() != 1 {
            bail!("wg peer has unexpected allowed IPs");
        }
        let address = allowed[0]
            .strip_suffix("/32")
            .context("wg peer is missing its IPv4 lease")?;
        valid_address(address)?;
        peers.push(PeerStatus {
            public_key: fields[0].to_owned(),
            address: address.to_owned(),
            latest_handshake_epoch_seconds: fields[4]
                .parse()
                .context("wg handshake counter is invalid")?,
            bytes_received: fields[5].parse().context("wg receive counter is invalid")?,
            bytes_sent: fields[6].parse().context("wg send counter is invalid")?,
        });
    }
    Ok(peers)
}

fn validate_peer(peer: &ManagedPeer) -> Result<()> {
    validate_key(peer.private_key.expose_secret(), "peer private key")?;
    validate_key(peer.preshared_key.expose_secret(), "peer preshared key")?;
    valid_address(&peer.address)
}

fn validate_key(key: &str, label: &str) -> Result<()> {
    if STANDARD
        .decode(key)
        .ok()
        .is_none_or(|decoded| decoded.len() != 32)
    {
        bail!("{label} is not a 32-byte standard-base64 WireGuard key");
    }
    Ok(())
}

fn valid_address(address: &str) -> Result<()> {
    let address = address
        .parse::<Ipv4Addr>()
        .context("peer address is not IPv4")?;
    let octets = address.octets();
    let host = (u16::from(octets[2]) << 8) | u16::from(octets[3]);
    if octets[..2] != [10, 66] || !(2..=65_534).contains(&host) {
        bail!("peer address is outside the Ket WireGuard lease pool");
    }
    Ok(())
}

fn validate_interface(interface: &str, label: &str) -> Result<()> {
    if interface.is_empty()
        || interface.len() > 15
        || !interface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("{label} is not a valid Linux interface name");
    }
    Ok(())
}

async fn run_command(program: &str, arguments: &[&str]) -> Result<Output> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to run {program}"))?;
    validate_output(program, output)
}

async fn run_secret_command(program: &str, arguments: &[&str], secret: &str) -> Result<Output> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to run {program}"))?;
    let input = Zeroizing::new(format!("{secret}\n"));
    child
        .stdin
        .take()
        .context("secret command stdin is unavailable")?
        .write_all(input.as_bytes())
        .await?;
    let output = child.wait_with_output().await?;
    validate_output(program, output)
}

fn validate_output(program: &str, output: Output) -> Result<Output> {
    if output.stdout.len() > MAX_COMMAND_OUTPUT_BYTES
        || output.stderr.len() > MAX_COMMAND_OUTPUT_BYTES
    {
        bail!("{program} returned too much output");
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed: {}", message.trim());
    }
    Ok(output)
}

fn invalid_peer(_error: anyhow::Error) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(AgentError {
            code: "invalid_peer",
            message: "peer request is invalid",
        }),
    )
}

fn internal_error(error: anyhow::Error) -> ApiError {
    tracing::error!(error = %error, "WireGuard agent operation failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AgentError {
            code: "wireguard_unavailable",
            message: "WireGuard operation failed",
        }),
    )
}

fn value(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn required(name: &str) -> Result<String> {
    let value = std::env::var(name).with_context(|| format!("{name} is required"))?;
    if value.trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(value)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wg_dump_parser_is_strict_and_maps_traffic_direction() {
        let dump = concat!(
            "private\tpublic\t51820\toff\n",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\t(none)\t198.51.100.2:1234\t10.66.0.2/32\t42\t100\t200\t0\n",
        );
        let peers = parse_peer_statuses(dump).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "10.66.0.2");
        assert_eq!(peers[0].bytes_received, 100);
        assert_eq!(peers[0].bytes_sent, 200);
        assert!(parse_peer_statuses("interface\nmalformed\n").is_err());
        assert!(valid_address("10.66.0.1").is_err());
        assert!(valid_address("10.66.0.2").is_ok());
    }
}
