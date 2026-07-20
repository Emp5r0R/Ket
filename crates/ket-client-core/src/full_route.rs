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
    net::TcpListener,
    process::{Child, Command},
    sync::{mpsc, watch},
    time::{sleep, timeout},
};

use crate::{ActiveTunnel, ClientError, TunnelStatus};

#[derive(Clone, Debug)]
pub(crate) struct FullRouteBridge {
    binary_path: PathBuf,
}

impl FullRouteBridge {
    pub(crate) fn new(binary_path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: binary_path.into(),
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
    ) -> Result<Child, ClientError> {
        Command::new(&self.binary_path)
            .args(arguments(socks_port, server_addresses))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                ClientError::transport(
                    transport_id,
                    "failed to launch the full-route bridge",
                    false,
                )
            })
    }
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
    ];
    let addresses = server_addresses
        .iter()
        .map(SocketAddr::ip)
        .collect::<BTreeSet<IpAddr>>();
    for address in addresses {
        arguments.push("--bypass".to_owned());
        arguments.push(address.to_string());
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
    bridge: &mut Child,
    settle_time: Duration,
    transport_id: &str,
) -> Result<(), ClientError> {
    let deadline = tokio::time::Instant::now() + settle_time;
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

pub(crate) async fn stop_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

pub(crate) fn supervise(
    transport_id: String,
    engine_name: &'static str,
    engine: Child,
    bridge: Child,
    stop_timeout: Duration,
) -> Arc<dyn ActiveTunnel> {
    let (command_tx, command_rx) = mpsc::channel(1);
    let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
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
        stop_timeout,
    })
}

enum ProcessCommand {
    Stop,
}

async fn supervise_processes(
    engine_name: &'static str,
    mut engine: Child,
    mut bridge: Child,
    mut commands: mpsc::Receiver<ProcessCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    tokio::select! {
        result = engine.wait() => {
            stop_child(&mut bridge).await;
            let message = result.map_or_else(
                |_| format!("failed to wait for {engine_name}"),
                |exit| format!("{engine_name} exited unexpectedly ({exit})"),
            );
            status.send_replace(TunnelStatus::Failed(message));
        }
        result = bridge.wait() => {
            stop_child(&mut engine).await;
            let message = result.map_or_else(
                |_| "failed to wait for the full-route bridge".to_owned(),
                |exit| format!("full-route bridge exited unexpectedly ({exit})"),
            );
            status.send_replace(TunnelStatus::Failed(message));
        }
        _ = commands.recv() => {
            stop_child(&mut bridge).await;
            stop_child(&mut engine).await;
            status.send_replace(TunnelStatus::Stopped);
        }
    }
}

struct FullRouteTunnel {
    transport_id: String,
    command_tx: mpsc::Sender<ProcessCommand>,
    status_rx: watch::Receiver<TunnelStatus>,
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
    use std::{net::Ipv4Addr, str::FromStr};

    use super::*;

    #[test]
    fn bridge_arguments_enforce_virtual_dns_and_all_server_bypasses() {
        let addresses = [
            SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 443)),
            SocketAddr::from_str("[2001:db8::9]:443").unwrap(),
            SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 8443)),
        ];

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
                "--bypass",
                "203.0.113.9",
                "--bypass",
                "2001:db8::9",
            ]
        );
    }
}
