use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    net::TcpListener,
    process::{Child, Command},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::{
    ActiveTunnel, ClientError, NetworkReport, TunnelStatus,
    leak_guard::LeakGuard,
    route_keeper::RouteKeeper,
    system_dns::{SystemDnsLease, SystemDnsManager},
};

/// The name Ket gives the bridge's tunnel interface, so the leak guard and
/// the system resolver can find it.
#[cfg(windows)]
pub(crate) const TUNNEL_INTERFACE: &str = "Ket";
#[cfg(not(windows))]
pub(crate) const TUNNEL_INTERFACE: &str = "ket0";

/// How long the bridge may take to create its tunnel interface.
const TUNNEL_INTERFACE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub(crate) struct FullRouteBridge {
    binary_path: PathBuf,
    dns: SystemDnsManager,
}

impl FullRouteBridge {
    pub(crate) fn new(binary_path: impl Into<PathBuf>, dns_state_path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: binary_path.into(),
            dns: SystemDnsManager::virtual_dns(dns_state_path),
        }
    }

    pub(crate) async fn check(&self, transport_id: &str) -> Result<(), ClientError> {
        let status = timeout(
            Duration::from_secs(5),
            Command::new(&self.binary_path)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await
        .map_err(|_| ClientError::transport(transport_id, "tun2proxy check timed out", true))?
        .map_err(|_| {
            ClientError::transport(transport_id, "tun2proxy executable is unavailable", false)
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(ClientError::transport(
                transport_id,
                "tun2proxy executable failed its version check",
                false,
            ))
        }
    }

    pub(crate) fn start(
        &self,
        socks_port: u16,
        server_addresses: &[SocketAddr],
        transport_id: &str,
    ) -> Result<FullRouteProcess, ClientError> {
        let dns_lease = self.dns.acquire().map_err(|_| {
            ClientError::transport(
                transport_id,
                "failed to route system DNS through the protected tunnel",
                false,
            )
        })?;
        let servers = server_addresses
            .iter()
            .map(SocketAddr::ip)
            .collect::<BTreeSet<IpAddr>>()
            .into_iter()
            .collect::<Vec<_>>();
        let leak_guard = LeakGuard::engage(&servers).map_err(|_| {
            ClientError::transport(
                transport_id,
                "failed to block DNS outside the protected tunnel",
                false,
            )
        })?;
        // Installed before the bridge routes everything into the tunnel, so
        // the engines never lose their direct path to the server.
        let route_keeper = RouteKeeper::engage(&servers, TUNNEL_INTERFACE).map_err(|_| {
            ClientError::transport(
                transport_id,
                "failed to keep the server reachable outside the protected tunnel",
                false,
            )
        })?;
        let mut child = Command::new(&self.binary_path)
            .args(arguments(socks_port, server_addresses))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    transport_id,
                    "failed to launch the full-route bridge",
                    false,
                )
            })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            let _ = child.start_kill();
            ClientError::transport(
                transport_id,
                "failed to capture full-route bridge diagnostics",
                false,
            )
        })?;
        Ok(FullRouteProcess {
            child,
            diagnostic: Some(tokio::spawn(collect_bridge_diagnostic(stderr))),
            dns_lease,
            leak_guard: Some(leak_guard),
            route_keeper: Some(route_keeper),
        })
    }
}

pub(crate) struct FullRouteProcess {
    child: Child,
    diagnostic: Option<JoinHandle<Option<BridgeDiagnostic>>>,
    dns_lease: SystemDnsLease,
    /// Released only after the bridge has stopped routing into the tunnel.
    leak_guard: Option<LeakGuard>,
    /// Keeps the engines' routes to the server on the current network; its
    /// routes are removed when it is dropped, after the bridge has stopped.
    route_keeper: Option<RouteKeeper>,
}

impl FullRouteProcess {
    /// Network changes under this tunnel. Without a route keeper nothing is
    /// ever reported.
    fn network(&self) -> watch::Receiver<NetworkReport> {
        self.route_keeper.as_ref().map_or_else(
            || watch::channel(NetworkReport::default()).1,
            RouteKeeper::network,
        )
    }

    /// Once the bridge has created its tunnel interface, admits it to the
    /// leak guard and routes the system resolver into it. `Ok(false)` while
    /// it does not exist yet.
    fn admit_tunnel(&mut self, transport_id: &str) -> Result<bool, ClientError> {
        let guarded = match self.leak_guard.as_mut() {
            Some(guard) => guard.admit_tunnel(TUNNEL_INTERFACE),
            None => Ok(()),
        };
        match guarded.and_then(|()| self.dns_lease.route_tunnel(TUNNEL_INTERFACE)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(ClientError::transport(
                transport_id,
                "failed to keep DNS inside the protected tunnel",
                false,
            )),
        }
    }
}

/// One settle-loop step for the bridge: fails if it exited or its tunnel
/// interface cannot be protected; `Ok(true)` once the guard admits it.
async fn bridge_settle_step(
    bridge: &mut FullRouteProcess,
    started: tokio::time::Instant,
    transport_id: &str,
) -> Result<bool, ClientError> {
    if let Some(message) = bridge_exit_if_stopped(bridge, transport_id).await? {
        return Err(ClientError::transport(transport_id, message, false));
    }
    if bridge.admit_tunnel(transport_id)? {
        return Ok(true);
    }
    if started.elapsed() >= TUNNEL_INTERFACE_TIMEOUT {
        return Err(ClientError::transport(
            transport_id,
            "the full-route bridge did not create its tunnel interface",
            false,
        ));
    }
    Ok(false)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BridgeDiagnostic {
    Permission,
    Wintun,
    TunDevice,
    DnsSetup,
    RouteSetup,
    AddressConflict,
    ResourceBusy,
    NetworkUnavailable,
    MissingComponent,
}

impl BridgeDiagnostic {
    fn message(self) -> &'static str {
        match self {
            Self::Permission => "permission was denied while configuring protected routes",
            Self::Wintun => "the Wintun driver could not be initialized",
            Self::TunDevice => "the TUN interface could not be created or retained",
            Self::DnsSetup => "protected DNS routing could not be configured",
            Self::RouteSetup => "system routes could not be configured",
            Self::AddressConflict => "a required local address or port is already in use",
            Self::ResourceBusy => "a required networking device is busy",
            Self::NetworkUnavailable => "the underlying network became unreachable",
            Self::MissingComponent => "a required operating-system networking component is missing",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Permission => 9,
            Self::Wintun => 8,
            Self::TunDevice => 7,
            Self::DnsSetup => 6,
            Self::RouteSetup => 5,
            Self::AddressConflict => 4,
            Self::ResourceBusy => 3,
            Self::NetworkUnavailable => 2,
            Self::MissingComponent => 1,
        }
    }
}

async fn collect_bridge_diagnostic(mut stderr: impl AsyncRead + Unpin) -> Option<BridgeDiagnostic> {
    const TAIL_BYTES: usize = 128;
    let mut chunk = [0_u8; 2_048];
    let mut tail = Vec::with_capacity(TAIL_BYTES);
    let mut selected = None;
    loop {
        let read = match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let mut scan = Vec::with_capacity(tail.len() + read);
        scan.extend_from_slice(&tail);
        scan.extend_from_slice(&chunk[..read]);
        let output = String::from_utf8_lossy(&scan);
        if let Some(candidate) = classify_bridge_diagnostic(&output)
            && selected
                .is_none_or(|current: BridgeDiagnostic| candidate.priority() > current.priority())
        {
            selected = Some(candidate);
        }
        let tail_start = scan.len().saturating_sub(TAIL_BYTES);
        tail.clear();
        tail.extend_from_slice(&scan[tail_start..]);
    }
    selected
}

fn classify_bridge_diagnostic(output: &str) -> Option<BridgeDiagnostic> {
    let normalized = output.to_ascii_lowercase();
    if contains_any(
        &normalized,
        &[
            "permission denied",
            "operation not permitted",
            "access is denied",
            "requires elevation",
            "administrator privileges",
        ],
    ) {
        Some(BridgeDiagnostic::Permission)
    } else if contains_any(&normalized, &["wintun", "wireguardnt"]) {
        Some(BridgeDiagnostic::Wintun)
    } else if contains_any(
        &normalized,
        &[
            "create tun",
            "open tun",
            "tun device",
            "tun interface",
            "tap device",
            "utun",
        ],
    ) {
        Some(BridgeDiagnostic::TunDevice)
    } else if contains_any(&normalized, &["dns", "resolv.conf", "systemd-resolved"]) {
        Some(BridgeDiagnostic::DnsSetup)
    } else if contains_any(
        &normalized,
        &["ip route", "ip rule", "netsh", "routing table", "add route"],
    ) {
        Some(BridgeDiagnostic::RouteSetup)
    } else if normalized.contains("address already in use") {
        Some(BridgeDiagnostic::AddressConflict)
    } else if contains_any(&normalized, &["device or resource busy", "resource busy"]) {
        Some(BridgeDiagnostic::ResourceBusy)
    } else if contains_any(
        &normalized,
        &[
            "network is unreachable",
            "no route to host",
            "network is down",
        ],
    ) {
        Some(BridgeDiagnostic::NetworkUnavailable)
    } else if contains_any(
        &normalized,
        &[
            "no such file or directory",
            "system cannot find the file specified",
            "module could not be found",
        ],
    ) {
        Some(BridgeDiagnostic::MissingComponent)
    } else {
        None
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn arguments(socks_port: u16, server_addresses: &[SocketAddr]) -> Vec<String> {
    let mut arguments = vec![
        "--setup".to_owned(),
        "--proxy".to_owned(),
        format!("socks5://127.0.0.1:{socks_port}"),
        "--dns".to_owned(),
        "virtual".to_owned(),
        "--verbosity".to_owned(),
        "error".to_owned(),
        "--exit-on-fatal-error".to_owned(),
        "--ipv6-enabled".to_owned(),
        "--tun".to_owned(),
        TUNNEL_INTERFACE.to_owned(),
    ];
    // On Windows the route keeper owns the server routes outright: the
    // bridge's own would point at the gateway of the network it started on.
    // On Linux the bridge finds the keeper's more specific routes and leaves
    // them alone.
    if !cfg!(windows) {
        let addresses = server_addresses
            .iter()
            .map(SocketAddr::ip)
            .collect::<BTreeSet<IpAddr>>();
        for address in addresses {
            arguments.push("--bypass".to_owned());
            arguments.push(address.to_string());
        }
    }
    arguments
}

pub(crate) async fn reserve_proxy_port(transport_id: &str) -> Result<u16, ClientError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|_| {
        ClientError::transport(transport_id, "failed to reserve a local proxy port", true)
    })?;
    Ok(listener
        .local_addr()
        .expect("bound listener has an address")
        .port())
}

pub(crate) async fn wait_until_stable(
    engine: &mut Child,
    engine_name: &str,
    bridge: &mut FullRouteProcess,
    settle_time: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let started = tokio::time::Instant::now();
    let deadline = started + settle_time;
    loop {
        if engine
            .try_wait()
            .map_err(|_| {
                ClientError::transport(
                    transport_id,
                    format!("failed to inspect the {engine_name} process"),
                    false,
                )
            })?
            .is_some()
        {
            return Err(ClientError::transport(
                transport_id,
                format!("{engine_name} stopped while enabling the full-route tunnel"),
                true,
            ));
        }
        let protected = bridge_settle_step(bridge, started, transport_id).await?;
        if protected && tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn wait_until_three_stable(
    engine: &mut Child,
    engine_name: &str,
    carrier: &mut Child,
    carrier_name: &str,
    bridge: &mut FullRouteProcess,
    settle_time: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let started = tokio::time::Instant::now();
    let deadline = started + settle_time;
    loop {
        for (child, name) in [(&mut *engine, engine_name), (&mut *carrier, carrier_name)] {
            if child
                .try_wait()
                .map_err(|_| {
                    ClientError::transport(
                        transport_id,
                        format!("failed to inspect the {name} process"),
                        false,
                    )
                })?
                .is_some()
            {
                return Err(ClientError::transport(
                    transport_id,
                    format!("{name} stopped while enabling the full-route tunnel"),
                    true,
                ));
            }
        }
        let protected = bridge_settle_step(bridge, started, transport_id).await?;
        if protected && tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn wait_until_pair_stable(
    engine: &mut Child,
    engine_name: &str,
    carrier: &mut Child,
    carrier_name: &str,
    settle_time: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let deadline = tokio::time::Instant::now() + settle_time;
    loop {
        for (child, name) in [(&mut *engine, engine_name), (&mut *carrier, carrier_name)] {
            if child
                .try_wait()
                .map_err(|_| {
                    ClientError::transport(transport_id, format!("failed to inspect {name}"), false)
                })?
                .is_some()
            {
                return Err(ClientError::transport(
                    transport_id,
                    format!("{name} stopped while enabling the tunnel"),
                    true,
                ));
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) async fn stop_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn stop_bridge_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(process_id) = child.id()
        && unsafe { libc::kill(process_id as libc::pid_t, libc::SIGINT) } == 0
        && matches!(
            timeout(Duration::from_secs(5), child.wait()).await,
            Ok(Ok(_))
        )
    {
        return;
    }

    stop_child(child).await;
}

pub(crate) async fn stop_bridge(bridge: &mut FullRouteProcess) -> std::io::Result<()> {
    // tun2proxy owns a Linux bind mount over /etc/resolv.conf. SIGINT lets its
    // cancellation path unmount it before Ket restores the underlying file.
    stop_bridge_child(&mut bridge.child).await;
    let _ = take_bridge_diagnostic(bridge).await;
    bridge.leak_guard = None;
    bridge.route_keeper = None;
    bridge.dns_lease.restore()
}

async fn bridge_exit_if_stopped(
    bridge: &mut FullRouteProcess,
    transport_id: &str,
) -> Result<Option<String>, ClientError> {
    let exit = bridge.child.try_wait().map_err(|_| {
        ClientError::transport(transport_id, "failed to inspect the route bridge", false)
    })?;
    let Some(exit) = exit else {
        return Ok(None);
    };
    let diagnostic = take_bridge_diagnostic(bridge).await;
    Ok(Some(bridge_exit_message(exit, diagnostic)))
}

async fn take_bridge_diagnostic(bridge: &mut FullRouteProcess) -> Option<BridgeDiagnostic> {
    let mut task = bridge.diagnostic.take()?;
    match timeout(Duration::from_millis(250), &mut task).await {
        Ok(Ok(diagnostic)) => diagnostic,
        Ok(Err(_)) => None,
        Err(_) => {
            task.abort();
            None
        }
    }
}

fn bridge_exit_message(
    exit: std::process::ExitStatus,
    diagnostic: Option<BridgeDiagnostic>,
) -> String {
    diagnostic.map_or_else(
        || format!("full-route bridge exited unexpectedly ({exit})"),
        |diagnostic| {
            format!(
                "full-route bridge exited unexpectedly ({exit}): {}",
                diagnostic.message()
            )
        },
    )
}

pub(crate) fn supervise(
    transport_id: String,
    engine_name: &'static str,
    engine: Child,
    bridge: FullRouteProcess,
    stop_timeout: Duration,
) -> Arc<dyn ActiveTunnel> {
    let (command_tx, command_rx) = mpsc::channel(1);
    let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
    let network = bridge.network();
    tokio::spawn(supervise_processes(
        engine_name,
        engine,
        bridge,
        command_rx,
        status_tx,
    ));
    Arc::new(FullRouteTunnel {
        transport_id,
        command_tx,
        status_rx,
        network,
        stop_timeout,
    })
}

/// Starts a carrier again with the arguments it first ran with.
pub(crate) type CarrierLauncher = Box<dyn Fn() -> std::io::Result<Child> + Send + Sync>;

/// Supervises an engine whose carrier holds one long connection to the
/// server (wstunnel for WireGuard). That connection dies with the network it
/// was made on, so the carrier is started again whenever the device moves to
/// another network; the engine and the tunnel interface stay up.
pub(crate) fn supervise_with_carrier(
    transport_id: String,
    engine_name: &'static str,
    engine: Child,
    carrier: Carrier,
    bridge: FullRouteProcess,
    stop_timeout: Duration,
) -> Arc<dyn ActiveTunnel> {
    let (command_tx, command_rx) = mpsc::channel(1);
    let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
    let network = bridge.network();
    tokio::spawn(supervise_three_processes(
        engine_name,
        engine,
        carrier,
        bridge,
        command_rx,
        status_tx,
    ));
    Arc::new(FullRouteTunnel {
        transport_id,
        command_tx,
        status_rx,
        network,
        stop_timeout,
    })
}

/// A running carrier process and how to start it again.
pub(crate) struct Carrier {
    pub(crate) name: &'static str,
    pub(crate) child: Child,
    pub(crate) relaunch: CarrierLauncher,
}

pub(crate) fn supervise_pair(
    transport_id: String,
    engine_name: &'static str,
    engine: Child,
    carrier_name: &'static str,
    carrier: Child,
    dns_lease: SystemDnsLease,
    stop_timeout: Duration,
) -> Arc<dyn ActiveTunnel> {
    let (command_tx, command_rx) = mpsc::channel(1);
    let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
    tokio::spawn(supervise_two_processes(
        engine_name,
        engine,
        carrier_name,
        carrier,
        dns_lease,
        command_rx,
        status_tx,
    ));
    Arc::new(FullRouteTunnel {
        transport_id,
        command_tx,
        status_rx,
        network: watch::channel(NetworkReport::default()).1,
        stop_timeout,
    })
}

enum ProcessCommand {
    Stop,
}

async fn supervise_processes(
    engine_name: &'static str,
    mut engine: Child,
    mut bridge: FullRouteProcess,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    tokio::select! {
        result = engine.wait() => {
            let cleanup = stop_bridge(&mut bridge).await;
            let mut message = result.map_or_else(
                |_| format!("failed to wait for {engine_name}"),
                |exit| format!("{engine_name} exited unexpectedly ({exit})"),
            );
            append_dns_cleanup_failure(&mut message, cleanup);
            status.send_replace(TunnelStatus::Failed(message));
        }
        result = bridge.child.wait() => {
            stop_child(&mut engine).await;
            let diagnostic = take_bridge_diagnostic(&mut bridge).await;
            let cleanup = bridge.dns_lease.restore();
            let mut message = result.map_or_else(
                |_| "failed to wait for the full-route bridge".to_owned(),
                |exit| bridge_exit_message(exit, diagnostic),
            );
            append_dns_cleanup_failure(&mut message, cleanup);
            status.send_replace(TunnelStatus::Failed(message));
        }
        _ = commands.recv() => {
            let cleanup = stop_bridge(&mut bridge).await;
            stop_child(&mut engine).await;
            publish_stopped_or_dns_failure(&status, cleanup);
        }
    }
}

async fn supervise_three_processes(
    engine_name: &'static str,
    mut engine: Child,
    mut carrier: Carrier,
    mut bridge: FullRouteProcess,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    let carrier_name = carrier.name;
    let mut network = bridge.network();
    let mut seen = network.borrow_and_update().generation;
    loop {
        tokio::select! {
            result = engine.wait() => {
                let cleanup = stop_bridge(&mut bridge).await;
                stop_child(&mut carrier.child).await;
                let mut message = result.map_or_else(
                    |_| format!("failed to wait for {engine_name}"),
                    |exit| format!("{engine_name} exited unexpectedly ({exit})"),
                );
                append_dns_cleanup_failure(&mut message, cleanup);
                status.send_replace(TunnelStatus::Failed(message));
                return;
            }
            result = carrier.child.wait() => {
                let cleanup = stop_bridge(&mut bridge).await;
                stop_child(&mut engine).await;
                let mut message = result.map_or_else(
                    |_| format!("failed to wait for {carrier_name}"),
                    |exit| format!("{carrier_name} exited unexpectedly ({exit})"),
                );
                append_dns_cleanup_failure(&mut message, cleanup);
                status.send_replace(TunnelStatus::Failed(message));
                return;
            }
            result = bridge.child.wait() => {
                stop_child(&mut carrier.child).await;
                stop_child(&mut engine).await;
                let diagnostic = take_bridge_diagnostic(&mut bridge).await;
                let cleanup = bridge.dns_lease.restore();
                let mut message = result.map_or_else(
                    |_| "failed to wait for the full-route bridge".to_owned(),
                    |exit| bridge_exit_message(exit, diagnostic),
                );
                append_dns_cleanup_failure(&mut message, cleanup);
                status.send_replace(TunnelStatus::Failed(message));
                return;
            }
            _ = commands.recv() => {
                let cleanup = stop_bridge(&mut bridge).await;
                stop_child(&mut carrier.child).await;
                stop_child(&mut engine).await;
                publish_stopped_or_dns_failure(&status, cleanup);
                return;
            }
            () = moved_to_another_network(&mut network, &mut seen) => {
                stop_child(&mut carrier.child).await;
                match (carrier.relaunch)() {
                    Ok(child) => carrier.child = child,
                    Err(_) => {
                        let cleanup = stop_bridge(&mut bridge).await;
                        stop_child(&mut engine).await;
                        let mut message =
                            format!("{carrier_name} could not start again on the new network");
                        append_dns_cleanup_failure(&mut message, cleanup);
                        status.send_replace(TunnelStatus::Failed(message));
                        return;
                    }
                }
            }
        }
    }
}

/// Returns once the device is on a network it was not on at `seen`; never
/// while it has no network at all.
async fn moved_to_another_network(network: &mut watch::Receiver<NetworkReport>, seen: &mut u64) {
    loop {
        if network.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
        let report = *network.borrow_and_update();
        if report.generation > *seen && !report.offline {
            *seen = report.generation;
            return;
        }
    }
}

async fn supervise_two_processes(
    engine_name: &'static str,
    mut engine: Child,
    carrier_name: &'static str,
    mut carrier: Child,
    mut dns_lease: SystemDnsLease,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    tokio::select! {
        result = engine.wait() => {
            stop_child(&mut carrier).await;
            let cleanup = dns_lease.restore();
            let mut message = result.map_or_else(
                |_| format!("failed to wait for {engine_name}"),
                |exit| format!("{engine_name} exited unexpectedly ({exit})"),
            );
            append_dns_cleanup_failure(&mut message, cleanup);
            status.send_replace(TunnelStatus::Failed(message));
        }
        result = carrier.wait() => {
            stop_child(&mut engine).await;
            let cleanup = dns_lease.restore();
            let mut message = result.map_or_else(
                |_| format!("failed to wait for {carrier_name}"),
                |exit| format!("{carrier_name} exited unexpectedly ({exit})"),
            );
            append_dns_cleanup_failure(&mut message, cleanup);
            status.send_replace(TunnelStatus::Failed(message));
        }
        _ = commands.recv() => {
            stop_child(&mut engine).await;
            stop_child(&mut carrier).await;
            let cleanup = dns_lease.restore();
            publish_stopped_or_dns_failure(&status, cleanup);
        }
    }
}

fn append_dns_cleanup_failure(message: &mut String, cleanup: std::io::Result<()>) {
    if cleanup.is_err() {
        message.push_str("; system DNS restoration failed");
    }
}

fn publish_stopped_or_dns_failure(
    status: &watch::Sender<TunnelStatus>,
    cleanup: std::io::Result<()>,
) {
    status.send_replace(if cleanup.is_ok() {
        TunnelStatus::Stopped
    } else {
        TunnelStatus::Failed("system DNS restoration failed after tunnel shutdown".to_owned())
    });
}

struct FullRouteTunnel {
    transport_id: String,
    command_tx: mpsc::Sender<ProcessCommand>,
    status_rx: watch::Receiver<TunnelStatus>,
    network: watch::Receiver<NetworkReport>,
    stop_timeout: Duration,
}

#[async_trait]
impl ActiveTunnel for FullRouteTunnel {
    fn transport_id(&self) -> &str {
        &self.transport_id
    }

    fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    fn network(&self) -> NetworkReport {
        *self.network.borrow()
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
        match &*status.borrow() {
            TunnelStatus::Stopped => Ok(()),
            TunnelStatus::Failed(message) => Err(ClientError::transport(
                &self.transport_id,
                message.clone(),
                true,
            )),
            TunnelStatus::Connected => Err(ClientError::transport(
                &self.transport_id,
                "transport shutdown did not complete",
                true,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, process::Stdio, str::FromStr};

    use super::*;

    #[test]
    fn bridge_arguments_enforce_virtual_dns_and_all_server_bypasses() {
        let addresses = [
            SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 443)),
            SocketAddr::from_str("[2001:db8::9]:443").unwrap(),
            SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 8443)),
        ];

        let bypasses: &[&str] = if cfg!(windows) {
            &[]
        } else {
            &["--bypass", "203.0.113.9", "--bypass", "2001:db8::9"]
        };
        assert_eq!(
            arguments(10808, &addresses),
            [
                "--setup",
                "--proxy",
                "socks5://127.0.0.1:10808",
                "--dns",
                "virtual",
                "--verbosity",
                "error",
                "--exit-on-fatal-error",
                "--ipv6-enabled",
                "--tun",
                TUNNEL_INTERFACE,
            ]
            .into_iter()
            .chain(bypasses.iter().copied())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn bridge_diagnostics_are_allowlisted_and_prioritized() {
        let cases = [
            (
                "fatal: operation not permitted",
                BridgeDiagnostic::Permission,
            ),
            (
                "failed to initialize wintun adapter",
                BridgeDiagnostic::Wintun,
            ),
            (
                "could not create TUN interface",
                BridgeDiagnostic::TunDevice,
            ),
            ("failed to replace resolv.conf", BridgeDiagnostic::DnsSetup),
            ("ip route add failed", BridgeDiagnostic::RouteSetup),
            ("address already in use", BridgeDiagnostic::AddressConflict),
            ("device or resource busy", BridgeDiagnostic::ResourceBusy),
            (
                "network is unreachable",
                BridgeDiagnostic::NetworkUnavailable,
            ),
            (
                "no such file or directory",
                BridgeDiagnostic::MissingComponent,
            ),
        ];
        for (stderr, expected) in cases {
            assert_eq!(classify_bridge_diagnostic(stderr), Some(expected));
        }
        assert_eq!(
            classify_bridge_diagnostic("dns setup failed: operation not permitted"),
            Some(BridgeDiagnostic::Permission),
        );
    }

    #[tokio::test]
    async fn bridge_diagnostic_collector_never_returns_raw_output() {
        let secret = "vless://user:credential@example.test/private-path";
        assert_eq!(collect_bridge_diagnostic(secret.as_bytes()).await, None,);

        let output = format!("{secret}\nfailed to open TUN device\n");
        let diagnostic = collect_bridge_diagnostic(output.as_bytes())
            .await
            .expect("the fixed TUN category should be detected");
        assert_eq!(diagnostic, BridgeDiagnostic::TunDevice);
        assert!(!diagnostic.message().contains("credential"));
        assert!(!diagnostic.message().contains("example.test"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bridge_shutdown_uses_graceful_signal_before_force_kill() {
        let mut child = Command::new("sh")
            .args(["-c", "trap 'exit 0' INT; while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        stop_bridge_child(&mut child).await;

        assert!(child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn stop_reports_terminal_cleanup_failure() {
        let (command_tx, mut command_rx) = mpsc::channel(1);
        let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
        tokio::spawn(async move {
            assert!(matches!(
                command_rx.recv().await,
                Some(ProcessCommand::Stop)
            ));
            status_tx.send_replace(TunnelStatus::Failed(
                "system DNS restoration failed after tunnel shutdown".to_owned(),
            ));
        });
        let tunnel = FullRouteTunnel {
            transport_id: "test-route".to_owned(),
            command_tx,
            status_rx,
            network: watch::channel(NetworkReport::default()).1,
            stop_timeout: Duration::from_secs(1),
        };

        let error = tunnel
            .stop()
            .await
            .expect_err("cleanup failure must surface");

        assert_eq!(error.issue().code, "transport_failed");
        assert!(error.issue().message.contains("DNS restoration failed"));
    }

    /// Runs scripts/dns-leak-check.ps1; its exit status, or `None` while the
    /// tunnel adapter is not up yet (status 2).
    #[cfg(windows)]
    fn windows_leak_check() -> Option<i32> {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/dns-leak-check.ps1");
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .output()
            .expect("run the Windows leak check");
        print!("{}", String::from_utf8_lossy(&output.stdout));
        match output.status.code() {
            Some(2) => None,
            code => code,
        }
    }

    #[cfg(windows)]
    async fn windows_leak_check_once_up() -> i32 {
        for _ in 0..60 {
            if let Some(code) = windows_leak_check() {
                return code;
            }
            sleep(Duration::from_millis(500)).await;
        }
        panic!("the tunnel adapter never came up");
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "routes this computer into a tunnel; run by Windows CI with KET_TEST_TUN2PROXY_BINARY"]
    async fn windows_bridge_keeps_lookups_inside_the_tunnel() {
        let binary = PathBuf::from(
            std::env::var_os("KET_TEST_TUN2PROXY_BINARY").expect("KET_TEST_TUN2PROXY_BINARY"),
        );
        // Relative paths are from the workspace root, as CI passes them.
        let binary = if binary.is_absolute() {
            binary
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(binary)
        };
        // Nothing listens on the proxy port: lookups are answered by the
        // bridge's virtual DNS, and nothing else needs to work.
        let proxy_port = reserve_proxy_port("leak-test").await.unwrap();

        // The bare bridge leaves every adapter's resolver reachable, which is
        // what Windows' parallel lookups use.
        let mut bare = Command::new(&binary)
            .args(arguments(proxy_port, &[]))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let bare_result = windows_leak_check_once_up().await;
        stop_child(&mut bare).await;
        sleep(Duration::from_secs(2)).await;
        assert_eq!(bare_result, 1, "the bare bridge was expected to leak");

        // Ket's bridge engages the leak guard around the same tunnel.
        let state = std::env::temp_dir().join(format!("ket-leak-test-{}", std::process::id()));
        let mut bridge = FullRouteBridge::new(&binary, &state)
            .start(proxy_port, &[], "leak-test")
            .unwrap();
        let started = tokio::time::Instant::now();
        while !bridge_settle_step(&mut bridge, started, "leak-test")
            .await
            .unwrap()
        {
            sleep(Duration::from_millis(100)).await;
        }
        let guarded_result = windows_leak_check_once_up().await;
        stop_bridge(&mut bridge).await.unwrap();
        assert_eq!(guarded_result, 0, "Ket's bridge leaked");
    }
}
