//! Headless connection for leak tests: enrolls from an invite read on stdin
//! and connects a full-route tunnel with the same client code the desktop
//! broker runs, then holds it until Ctrl-C or SIGTERM and disconnects.
//!
//!   connect_check --engines DIR --runtime DIR [--protocol NAME] [--split-tls-hello]
//!                 [--bypass PREFIX]... [--maintain]
//!
//! `--engines` holds the engine binaries under their installed names
//! (`xray`, `hysteria`, `sslocal`, `tun2proxy`, `wstunnel`, `openvpn`,
//! `stunnel`); missing engines simply drop their transports. `--protocol`
//! takes a contract name such as `vless_xtls_reality`; `--split-tls-hello`
//! turns on the setting of that name; `--bypass` keeps an address or network
//! outside the tunnel, as the bypass list does; `--maintain` runs the apps' maintenance
//! loop (renewal and automatic recovery) and prints `phase=…` on every change,
//! so a test can tell a seamless network change from a reconnect. Needs root on Linux,
//! like the broker (Administrator on Windows). Prints `connected protocol=…` once the route is up and
//! `disconnected` after release; exits non-zero on any failure.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ket_client_core::{
    BypassList, ConnectOptions, HttpControlPlane, Hysteria2Adapter, InviteEnrollment, IpPrefix,
    KetClient, OpenVpnStunnelAdapter, SelectionPolicy, Shadowsocks2022Adapter, TransportAdapter,
    WireGuardTlsAdapter, XrayAdapter,
};
use ket_core::TransportProtocol;

struct Options {
    engines: PathBuf,
    runtime: PathBuf,
    protocol: Option<TransportProtocol>,
    connect: ConnectOptions,
    maintain: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("connect_check: {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options() -> Result<Options, String> {
    let mut engines = None;
    let mut runtime = None;
    let mut protocol = None;
    let mut connect = ConnectOptions::default();
    let mut maintain = false;
    let mut bypass = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--engines" => engines = Some(PathBuf::from(value()?)),
            "--runtime" => runtime = Some(PathBuf::from(value()?)),
            "--protocol" => {
                let name = value()?;
                protocol = Some(
                    serde_json::from_value(serde_json::Value::String(name.clone()))
                        .map_err(|_| format!("unknown protocol: {name}"))?,
                );
            }
            "--split-tls-hello" => connect.split_tls_hello = true,
            "--bypass" => {
                let entry = value()?;
                bypass.push(
                    IpPrefix::parse_bypass(&entry)
                        .map_err(|error| format!("--bypass {entry}: {error}"))?,
                );
            }
            "--maintain" => maintain = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    connect.bypass = BypassList::new(bypass).map_err(|error| format!("--bypass: {error}"))?;
    Ok(Options {
        engines: engines.ok_or("--engines is required")?,
        runtime: runtime.ok_or("--runtime is required")?,
        protocol,
        connect,
        maintain,
    })
}

/// The broker's adapters, built from whichever engines are present.
fn adapters(engines: &Path, runtime: &Path) -> Vec<Arc<dyn TransportAdapter>> {
    let engine = |name: &str| engines.join(name);
    let present = |name: &str| engine(name).is_file();
    let dns_state = runtime.join("resolver.state");
    let mut adapters: Vec<Arc<dyn TransportAdapter>> = Vec::new();
    if !present("tun2proxy") {
        return adapters;
    }
    let bridge = engine("tun2proxy");
    if present("hysteria") {
        adapters.push(Arc::new(Hysteria2Adapter::new(
            engine("hysteria"),
            &bridge,
            runtime,
            &dns_state,
        )));
    }
    if present("xray") {
        adapters.push(Arc::new(XrayAdapter::new(
            engine("xray"),
            &bridge,
            runtime,
            &dns_state,
        )));
    }
    if present("sslocal") {
        adapters.push(Arc::new(Shadowsocks2022Adapter::new(
            engine("sslocal"),
            &bridge,
            runtime,
            &dns_state,
        )));
    }
    if present("openvpn") && present("stunnel") {
        adapters.push(Arc::new(OpenVpnStunnelAdapter::new(
            engine("openvpn"),
            engine("stunnel"),
            runtime,
            &dns_state,
        )));
    }
    if present("xray") && present("wstunnel") {
        adapters.push(Arc::new(WireGuardTlsAdapter::new(
            engine("xray"),
            engine("wstunnel"),
            &bridge,
            runtime,
            &dns_state,
        )));
    }
    adapters
}

async fn run() -> Result<(), String> {
    let options = parse_options()?;
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|error| format!("could not read stdin: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let enrollment = InviteEnrollment::from_text(input.trim(), now)
        .map_err(|error| format!("invite: {error}"))?;
    let adapters = adapters(&options.engines, &options.runtime);
    if adapters.is_empty() {
        return Err(format!("no engines in {}", options.engines.display()));
    }
    let plane = HttpControlPlane::new().map_err(|error| format!("client: {error}"))?;
    let client = KetClient::new(
        enrollment.endpoint,
        "leak check",
        Arc::new(plane),
        adapters,
        SelectionPolicy::default(),
    )
    .map_err(|error| format!("client: {error}"))?;
    client
        .enroll(enrollment.access_code)
        .await
        .map_err(|error| format!("enrollment: {error}"))?;

    client.set_connect_options(options.connect).await;
    let connected = client.connect_with_protocol(options.protocol).await;
    let held = match &connected {
        Ok(snapshot) => {
            let protocol = snapshot
                .active_transport
                .as_ref()
                .map(|transport| format!("{:?}", transport.protocol))
                .unwrap_or_else(|| "unknown".to_owned());
            println!("connected protocol={protocol}");
            let _maintenance = options.maintain.then(|| {
                let mut snapshots = client.subscribe();
                tokio::spawn(async move {
                    let mut last = None;
                    while snapshots.changed().await.is_ok() {
                        let phase = snapshots.borrow_and_update().phase.clone();
                        if last.as_ref() != Some(&phase) {
                            println!("phase={phase:?}");
                            last = Some(phase);
                        }
                    }
                });
                client.spawn_maintenance(std::time::Duration::from_secs(5))
            });
            wait_for_stop().await
        }
        Err(_) => Ok(()),
    };
    let disconnected = client.disconnect().await;
    connected.map_err(|error| format!("connect: {error}"))?;
    held?;
    disconnected.map_err(|error| format!("disconnect: {error}"))?;
    println!("disconnected");
    Ok(())
}

#[cfg(unix)]
async fn wait_for_stop() -> Result<(), String> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|error| format!("signal: {error}"))?;
    tokio::select! {
        _ = terminate.recv() => Ok(()),
        interrupted = tokio::signal::ctrl_c() => interrupted.map_err(|error| format!("signal: {error}")),
    }
}

#[cfg(not(unix))]
async fn wait_for_stop() -> Result<(), String> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| format!("signal: {error}"))
}
