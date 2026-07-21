use std::{
    collections::{BTreeMap, BTreeSet},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
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
use subtle::ConstantTimeEq;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UnixStream},
    process::{Child, Command},
    sync::{Mutex, watch},
    time::{sleep, timeout},
};

use crate::openvpn::{
    OpenVpnSessionStatus, ReconcileOpenVpnSessions, RemoveOpenVpnSessions, valid_session_username,
};

const MAX_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_MANAGEMENT_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MANAGEMENT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct AgentConfig {
    bind_address: SocketAddr,
    manager_token: String,
    openvpn_binary: PathBuf,
    server_config: PathBuf,
    management_socket: PathBuf,
    tunnel_interface: String,
    egress_interface: String,
    max_clients: usize,
}

impl AgentConfig {
    fn from_env() -> Result<Self> {
        let bind_address = value("KET_OPENVPN_AGENT_BIND", "0.0.0.0:8789")
            .parse()
            .context("KET_OPENVPN_AGENT_BIND must be a socket address")?;
        let manager_token = required("KET_OPENVPN_MANAGER_TOKEN")?;
        if manager_token.len() < 32 {
            bail!("KET_OPENVPN_MANAGER_TOKEN must contain at least 32 characters");
        }
        let openvpn_binary = PathBuf::from(value("KET_OPENVPN_BINARY", "/usr/local/bin/openvpn"));
        let server_config = PathBuf::from(value(
            "KET_OPENVPN_SERVER_CONFIG",
            "/etc/openvpn/ket-server.conf",
        ));
        let management_socket = PathBuf::from(value(
            "KET_OPENVPN_MANAGEMENT_SOCKET",
            "/run/ket-openvpn/management.sock",
        ));
        if !management_socket.is_absolute() {
            bail!("KET_OPENVPN_MANAGEMENT_SOCKET must be an absolute path");
        }
        let tunnel_interface = value("KET_OPENVPN_TUN_INTERFACE", "ketovpn0");
        let egress_interface = value("KET_OPENVPN_EGRESS_INTERFACE", "eth0");
        validate_interface(&tunnel_interface, "KET_OPENVPN_TUN_INTERFACE")?;
        validate_interface(&egress_interface, "KET_OPENVPN_EGRESS_INTERFACE")?;
        let max_clients = value("KET_OPENVPN_MAX_CLIENTS", "1000")
            .parse::<usize>()
            .ok()
            .filter(|count| (1..=65_533).contains(count))
            .context("KET_OPENVPN_MAX_CLIENTS must be between 1 and 65533")?;
        Ok(Self {
            bind_address,
            manager_token,
            openvpn_binary,
            server_config,
            management_socket,
            tunnel_interface,
            egress_interface,
            max_clients,
        })
    }
}

#[derive(Clone)]
struct AgentState {
    config: Arc<AgentConfig>,
    child: Arc<Mutex<Child>>,
    running: Arc<AtomicBool>,
}

#[derive(serde::Serialize)]
struct AgentError {
    code: &'static str,
    message: &'static str,
}

type ApiError = (StatusCode, Json<AgentError>);

pub async fn run() -> Result<()> {
    let config = Arc::new(AgentConfig::from_env()?);
    validate_files(&config).await?;
    install_firewall(&config).await?;
    prepare_management_socket(&config.management_socket).await?;
    let child = launch_openvpn(&config).await?;
    let state = AgentState {
        config: Arc::clone(&config),
        child: Arc::new(Mutex::new(child)),
        running: Arc::new(AtomicBool::new(true)),
    };
    if let Err(error) = wait_until_ready(&state).await {
        stop_openvpn(&state).await;
        return Err(error);
    }

    let listener = TcpListener::bind(config.bind_address)
        .await
        .with_context(|| format!("failed to bind OpenVPN agent on {}", config.bind_address))?;
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/reconcile", put(reconcile_sessions))
        .route("/v1/sessions/remove", post(remove_sessions))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state.clone());
    let (stopped_tx, mut stopped_rx) = watch::channel(false);
    tokio::spawn(watch_openvpn(state.clone(), stopped_tx));
    tracing::info!(address = %config.bind_address, "OpenVPN agent ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                () = shutdown_signal() => {}
                _ = stopped_rx.changed() => {}
            }
        })
        .await
        .context("OpenVPN agent stopped unexpectedly")?;
    let exited = !state.running.load(Ordering::Acquire);
    stop_openvpn(&state).await;
    if exited {
        bail!("OpenVPN exited unexpectedly");
    }
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
    let expected = state.config.manager_token.as_bytes();
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
    if !state.running.load(Ordering::Acquire) {
        return Err(unavailable(anyhow::anyhow!("OpenVPN is not running")));
    }
    management_command(&state.config.management_socket, "pid")
        .await
        .and_then(|response| {
            if response.contains("SUCCESS: pid=") {
                Ok(StatusCode::NO_CONTENT)
            } else {
                bail!("OpenVPN management did not return a process ID")
            }
        })
        .map_err(unavailable)
}

async fn list_sessions(
    State(state): State<AgentState>,
) -> Result<Json<Vec<OpenVpnSessionStatus>>, ApiError> {
    session_statuses(&state.config.management_socket)
        .await
        .map(Json)
        .map_err(unavailable)
}

async fn reconcile_sessions(
    State(state): State<AgentState>,
    Json(request): Json<ReconcileOpenVpnSessions>,
) -> Result<StatusCode, ApiError> {
    let desired = validate_usernames(request.usernames, state.config.max_clients)?;
    let current = session_statuses(&state.config.management_socket)
        .await
        .map_err(unavailable)?;
    let stale = current
        .into_iter()
        .map(|status| status.username)
        .filter(|username| !desired.contains(username))
        .collect::<BTreeSet<_>>();
    kill_usernames(&state.config.management_socket, &stale)
        .await
        .map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_sessions(
    State(state): State<AgentState>,
    Json(request): Json<RemoveOpenVpnSessions>,
) -> Result<StatusCode, ApiError> {
    let requested = validate_usernames(request.usernames, state.config.max_clients)?;
    let active = session_statuses(&state.config.management_socket)
        .await
        .map_err(unavailable)?
        .into_iter()
        .map(|status| status.username)
        .collect::<BTreeSet<_>>();
    let present = requested.intersection(&active).cloned().collect();
    kill_usernames(&state.config.management_socket, &present)
        .await
        .map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

fn validate_usernames(
    usernames: Vec<String>,
    maximum: usize,
) -> Result<BTreeSet<String>, ApiError> {
    if usernames.len() > maximum {
        return Err(invalid_request());
    }
    let usernames = usernames.into_iter().collect::<BTreeSet<_>>();
    if usernames.len() > maximum || usernames.iter().any(|name| !valid_session_username(name)) {
        return Err(invalid_request());
    }
    Ok(usernames)
}

async fn kill_usernames(socket: &Path, usernames: &BTreeSet<String>) -> Result<()> {
    for username in usernames {
        let response = management_command(socket, &format!("kill {username}")).await?;
        if !response.contains("SUCCESS:") {
            bail!("OpenVPN management rejected a session removal");
        }
    }
    Ok(())
}

async fn validate_files(config: &AgentConfig) -> Result<()> {
    for (path, label) in [
        (&config.openvpn_binary, "OpenVPN binary"),
        (&config.server_config, "OpenVPN server configuration"),
    ] {
        let metadata = fs::metadata(path)
            .await
            .with_context(|| format!("{label} is unavailable at {}", path.display()))?;
        if !metadata.is_file() {
            bail!("{label} is not a regular file");
        }
    }
    let output = timeout(
        Duration::from_secs(5),
        Command::new(&config.openvpn_binary)
            .arg("--version")
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .context("OpenVPN version check timed out")??;
    validate_output("openvpn", output)?;
    Ok(())
}

async fn prepare_management_socket(path: &Path) -> Result<()> {
    let parent = path.parent().context("management socket has no parent")?;
    fs::create_dir_all(parent).await?;
    if fs::symlink_metadata(path).await.is_ok() {
        fs::remove_file(path).await?;
    }
    Ok(())
}

async fn launch_openvpn(config: &AgentConfig) -> Result<Child> {
    Command::new(&config.openvpn_binary)
        .arg("--config")
        .arg(&config.server_config)
        .arg("--management")
        .arg(&config.management_socket)
        .arg("unix")
        .arg("--dev")
        .arg(&config.tunnel_interface)
        .arg("--max-clients")
        .arg(config.max_clients.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("failed to launch OpenVPN")
}

async fn wait_until_ready(state: &AgentState) -> Result<()> {
    for _ in 0..100 {
        if state.child.lock().await.try_wait()?.is_some() {
            state.running.store(false, Ordering::Release);
            bail!("OpenVPN exited during startup");
        }
        if management_command(&state.config.management_socket, "pid")
            .await
            .is_ok_and(|response| response.contains("SUCCESS: pid="))
        {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    bail!("OpenVPN management socket did not become ready")
}

async fn watch_openvpn(state: AgentState, stopped: watch::Sender<bool>) {
    loop {
        sleep(Duration::from_secs(1)).await;
        let exited = state.child.lock().await.try_wait().ok().flatten().is_some();
        if exited {
            state.running.store(false, Ordering::Release);
            stopped.send_replace(true);
            break;
        }
    }
}

async fn stop_openvpn(state: &AgentState) {
    let mut child = state.child.lock().await;
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.start_kill();
        let _ = timeout(Duration::from_secs(8), child.wait()).await;
    }
}

async fn management_command(socket: &Path, command: &str) -> Result<String> {
    if command.is_empty()
        || command.len() > 64
        || command
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
    {
        bail!("invalid OpenVPN management command");
    }
    timeout(MANAGEMENT_TIMEOUT, async {
        let mut stream = UnixStream::connect(socket).await?;
        stream
            .write_all(format!("{command}\nquit\n").as_bytes())
            .await?;
        stream.shutdown().await?;
        let mut output = Vec::new();
        stream
            .take(MAX_MANAGEMENT_RESPONSE_BYTES + 1)
            .read_to_end(&mut output)
            .await?;
        if output.len() as u64 > MAX_MANAGEMENT_RESPONSE_BYTES {
            bail!("OpenVPN management returned too much data");
        }
        String::from_utf8(output).context("OpenVPN management returned non-UTF-8 data")
    })
    .await
    .context("OpenVPN management operation timed out")?
}

async fn session_statuses(socket: &Path) -> Result<Vec<OpenVpnSessionStatus>> {
    let response = management_command(socket, "status 3").await?;
    parse_status(&response)
}

fn parse_status(response: &str) -> Result<Vec<OpenVpnSessionStatus>> {
    let mut header = None;
    let mut rows = Vec::new();
    for line in response.lines() {
        let fields = parse_csv_line(line)?;
        if fields.first().map(String::as_str) == Some("HEADER")
            && fields.get(1).map(String::as_str) == Some("CLIENT_LIST")
        {
            header = Some(
                fields
                    .iter()
                    .enumerate()
                    .skip(2)
                    .map(|(index, field)| (field.clone(), index - 1))
                    .collect::<BTreeMap<_, _>>(),
            );
        } else if fields.first().map(String::as_str) == Some("CLIENT_LIST") {
            rows.push(fields);
        }
    }
    let header = header.context("OpenVPN status omitted the client header")?;
    let index = |name: &str| {
        header
            .get(name)
            .copied()
            .with_context(|| format!("OpenVPN status omitted {name}"))
    };
    let username = index("Username")?;
    let virtual_address = index("Virtual Address")?;
    let connected_since = index("Connected Since (time_t)")?;
    let bytes_received = index("Bytes Received")?;
    let bytes_sent = index("Bytes Sent")?;
    rows.into_iter()
        .map(|fields| {
            let field = |position: usize| {
                fields
                    .get(position)
                    .map(String::as_str)
                    .context("OpenVPN status row is truncated")
            };
            let username = field(username)?.to_owned();
            if !valid_session_username(&username) {
                bail!("OpenVPN status contains an unmanaged username");
            }
            let virtual_address = field(virtual_address)?.to_owned();
            let address = virtual_address
                .parse::<Ipv4Addr>()
                .context("OpenVPN status contains an invalid virtual address")?;
            if address.octets()[..2] != [10, 67] {
                bail!("OpenVPN status contains an address outside the lease network");
            }
            Ok(OpenVpnSessionStatus {
                username,
                virtual_address,
                connected_since_epoch_seconds: field(connected_since)?
                    .parse()
                    .context("OpenVPN connected timestamp is invalid")?,
                bytes_received: field(bytes_received)?
                    .parse()
                    .context("OpenVPN receive counter is invalid")?,
                bytes_sent: field(bytes_sent)?
                    .parse()
                    .context("OpenVPN send counter is invalid")?,
            })
        })
        .collect()
}

fn parse_csv_line(line: &str) -> Result<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            character => field.push(character),
        }
    }
    if quoted {
        bail!("OpenVPN status contains unterminated CSV quoting");
    }
    fields.push(field);
    Ok(fields)
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
            &config.tunnel_interface,
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
        &config.tunnel_interface,
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
    ensure_iptables(&["FORWARD", "-i", &config.tunnel_interface, "-j", "ACCEPT"]).await?;
    ensure_iptables(&[
        "FORWARD",
        "-o",
        &config.tunnel_interface,
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
            "10.67.0.0/16",
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

async fn run_command(program: &str, arguments: &[&str]) -> Result<Output> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to run {program}"))?;
    validate_output(program, output)
}

fn validate_output(program: &str, output: Output) -> Result<Output> {
    if output.stdout.len().saturating_add(output.stderr.len()) > MAX_COMMAND_OUTPUT_BYTES {
        bail!("{program} returned too much output");
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed: {}", message.trim());
    }
    Ok(output)
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

fn invalid_request() -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(AgentError {
            code: "invalid_session_request",
            message: "session request is invalid",
        }),
    )
}

fn unavailable(error: anyhow::Error) -> ApiError {
    tracing::error!(error = %error, "OpenVPN agent operation failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AgentError {
            code: "openvpn_unavailable",
            message: "OpenVPN operation failed",
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
    fn status_v3_parser_is_header_driven_and_strict() {
        let status = concat!(
            ">INFO:OpenVPN Management Interface Version 5 -- type 'help' for more info\n",
            "TITLE,OpenVPN 2.7.4 x86_64\n",
            "HEADER,CLIENT_LIST,Common Name,Real Address,Virtual Address,Virtual IPv6 Address,Bytes Received,Bytes Sent,Connected Since,Connected Since (time_t),Username,Client ID,Peer ID,Data Channel Cipher\n",
            "CLIENT_LIST,AbCdEf123456,198.51.100.1:1234,10.67.0.2,,42,19,2026-07-21 00:00:00,1784592000,AbCdEf123456,0,0,AES-256-GCM\n",
            "END\n",
        );
        let sessions = parse_status(status).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].username, "AbCdEf123456");
        assert_eq!(sessions[0].virtual_address, "10.67.0.2");
        assert_eq!(sessions[0].bytes_received, 42);
        assert_eq!(sessions[0].bytes_sent, 19);
        assert!(parse_status("END\n").is_err());
        assert!(parse_status(&status.replace("10.67.0.2", "192.168.1.2")).is_err());
        assert!(parse_status(&status.replace("AbCdEf123456", "../../etc/pass")).is_err());
    }

    #[test]
    fn csv_parser_handles_management_quoting() {
        assert_eq!(
            parse_csv_line("CLIENT_LIST,\"name,with,commas\",\"quoted\"\"value\"").unwrap(),
            ["CLIENT_LIST", "name,with,commas", "quoted\"value"]
        );
        assert!(parse_csv_line("\"unterminated").is_err());
    }
}
