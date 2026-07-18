use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use ket_client_core::{
    ActiveTunnel, Hysteria2Adapter, HysteriaTunSettings, TransportAdapter, XrayRealityAdapter,
};
use ket_tunnel_protocol::{
    BrokerFault, BrokerRequest, BrokerResponse, BrokerToken, BrokerTunnelStatus, HandshakeProof,
    HandshakeResult, challenge, read_frame, write_frame,
};
use rand::{Rng, distributions::Alphanumeric};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore},
    time::timeout,
};

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONNECTIONS: usize = 8;

#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub address: SocketAddr,
    pub token_file: PathBuf,
    pub hysteria_path: PathBuf,
    pub xray_path: PathBuf,
    pub bridge_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub lease_ttl: Duration,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self> {
        let address = std::env::var("KET_BROKER_ADDRESS")
            .unwrap_or_else(|_| ket_tunnel_protocol::DEFAULT_BROKER_ADDRESS.to_owned())
            .parse::<SocketAddr>()
            .context("KET_BROKER_ADDRESS must be a socket address")?;
        if !address.ip().is_loopback() {
            bail!("KET_BROKER_ADDRESS must use a loopback IP address");
        }
        let token_file = std::env::var_os("KET_BROKER_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(default_token_file);
        let hysteria_path = std::env::var_os("KET_HYSTERIA_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(default_hysteria_path);
        let xray_path = std::env::var_os("KET_XRAY_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(default_xray_path);
        let bridge_path = std::env::var_os("KET_TUN2PROXY_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(default_bridge_path);
        let runtime_dir = std::env::var_os("KET_BROKER_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_runtime_dir);
        Ok(Self {
            address,
            token_file,
            hysteria_path,
            xray_path,
            bridge_path,
            runtime_dir,
            lease_ttl: Duration::from_secs(12),
        })
    }
}

struct TunnelLease {
    tunnel: Arc<dyn ActiveTunnel>,
    expires_at: Instant,
}

pub struct BrokerService {
    adapters: Vec<Arc<dyn TransportAdapter>>,
    engine_available: bool,
    lease_ttl: Duration,
    operation: Mutex<()>,
    tunnels: Mutex<HashMap<String, TunnelLease>>,
}

impl BrokerService {
    pub fn new(
        adapters: Vec<Arc<dyn TransportAdapter>>,
        engine_available: bool,
        lease_ttl: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            adapters,
            engine_available,
            lease_ttl: lease_ttl.max(Duration::from_secs(6)),
            operation: Mutex::new(()),
            tunnels: Mutex::new(HashMap::new()),
        })
    }

    async fn handle(&self, request: BrokerRequest) -> BrokerResponse {
        match request {
            BrokerRequest::Ping => BrokerResponse::Pong {
                engine_available: self.engine_available,
            },
            BrokerRequest::Probe { transport } => {
                let Some(adapter) = self.adapter_for(&transport) else {
                    return fault("unsupported_transport", "transport is not supported", false);
                };
                match adapter.probe(&transport).await {
                    Ok(report) => BrokerResponse::Probe {
                        resolved_addresses: report.resolved_addresses,
                        elapsed_ms: millis(report.elapsed),
                    },
                    Err(error) => client_fault(error),
                }
            }
            BrokerRequest::Connect {
                transport,
                resolved_addresses,
            } => {
                let Some(adapter) = self.adapter_for(&transport) else {
                    return fault("unsupported_transport", "transport is not supported", false);
                };
                let _operation = self.operation.lock().await;
                self.reap_expired().await;
                if resolved_addresses.is_empty() {
                    return fault("invalid_transport", "transport request is invalid", false);
                }
                if !self.tunnels.lock().await.is_empty() {
                    return fault(
                        "tunnel_busy",
                        "another full-route tunnel is already active",
                        true,
                    );
                }
                let probe = ket_client_core::ProbeReport {
                    resolved_addresses,
                    elapsed: Duration::ZERO,
                };
                match adapter.connect(&transport, &probe).await {
                    Ok(started) => {
                        let tunnel_id = random_id();
                        self.tunnels.lock().await.insert(
                            tunnel_id.clone(),
                            TunnelLease {
                                tunnel: started.tunnel,
                                expires_at: Instant::now() + self.lease_ttl,
                            },
                        );
                        BrokerResponse::Connected {
                            tunnel_id,
                            handshake_latency_ms: millis(started.handshake_latency),
                        }
                    }
                    Err(error) => client_fault(error),
                }
            }
            BrokerRequest::Status { tunnel_id } => self.tunnel_status(&tunnel_id, false).await,
            BrokerRequest::Heartbeat { tunnel_id } => self.tunnel_status(&tunnel_id, true).await,
            BrokerRequest::Stop { tunnel_id } => {
                let _operation = self.operation.lock().await;
                let lease = self.tunnels.lock().await.remove(&tunnel_id);
                let Some(lease) = lease else {
                    return fault("tunnel_not_found", "tunnel is no longer active", false);
                };
                match lease.tunnel.stop().await {
                    Ok(()) => BrokerResponse::Stopped,
                    Err(error) => client_fault(error),
                }
            }
        }
    }

    async fn tunnel_status(&self, tunnel_id: &str, heartbeat: bool) -> BrokerResponse {
        let mut tunnels = self.tunnels.lock().await;
        let Some(lease) = tunnels.get(tunnel_id) else {
            return fault("tunnel_not_found", "tunnel is no longer active", false);
        };
        let status = lease.tunnel.status().borrow().clone();
        if matches!(status, ket_client_core::TunnelStatus::Connected) {
            if heartbeat {
                tunnels
                    .get_mut(tunnel_id)
                    .expect("the tunnel lease remains present while locked")
                    .expires_at = Instant::now() + self.lease_ttl;
            }
        } else {
            tunnels.remove(tunnel_id);
        }
        BrokerResponse::Tunnel {
            status: match status {
                ket_client_core::TunnelStatus::Connected => BrokerTunnelStatus::Connected,
                ket_client_core::TunnelStatus::Stopped => BrokerTunnelStatus::Stopped,
                ket_client_core::TunnelStatus::Failed(message) => {
                    BrokerTunnelStatus::Failed(message)
                }
            },
        }
    }

    fn adapter_for(
        &self,
        transport: &ket_core::SessionTransport,
    ) -> Option<Arc<dyn TransportAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.supports(transport))
            .cloned()
    }

    async fn reap_expired(&self) {
        let now = Instant::now();
        let expired = {
            let mut tunnels = self.tunnels.lock().await;
            let ids = tunnels
                .iter()
                .filter_map(|(id, lease)| (lease.expires_at <= now).then_some(id.clone()))
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| tunnels.remove(&id))
                .collect::<Vec<_>>()
        };
        for lease in expired {
            let _ = lease.tunnel.stop().await;
        }
    }

    async fn stop_all(&self) {
        let leases = self
            .tunnels
            .lock()
            .await
            .drain()
            .map(|(_, lease)| lease)
            .collect::<Vec<_>>();
        for lease in leases {
            let _ = lease.tunnel.stop().await;
        }
    }
}

pub async fn serve_until(
    config: ServiceConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    let token = Arc::new(
        BrokerToken::load(&config.token_file)
            .context("failed to load the tunnel broker installation token")?,
    );
    let mut adapters: Vec<Arc<dyn TransportAdapter>> = Vec::with_capacity(2);
    if config.hysteria_path.is_file() {
        adapters.push(Arc::new(Hysteria2Adapter::new(
            &config.hysteria_path,
            &config.runtime_dir,
            HysteriaTunSettings::default(),
        )));
    }
    if config.xray_path.is_file() && config.bridge_path.is_file() {
        adapters.push(Arc::new(XrayRealityAdapter::new(
            &config.xray_path,
            &config.bridge_path,
            &config.runtime_dir,
        )));
    }
    let engine_available = !adapters.is_empty();
    let service = BrokerService::new(adapters, engine_available, config.lease_ttl);
    let listener = TcpListener::bind(config.address)
        .await
        .with_context(|| format!("failed to bind tunnel broker on {}", config.address))?;
    let concurrency = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let reaper = spawn_reaper(Arc::clone(&service));
    tokio::pin!(shutdown);
    tracing::info!(address = %config.address, "Ket tunnel service ready");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("failed to accept tunnel broker client")?;
                if !peer.ip().is_loopback() {
                    tracing::warn!(%peer, "rejected non-loopback tunnel broker client");
                    continue;
                }
                let permit = match Arc::clone(&concurrency).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => continue,
                };
                let token = Arc::clone(&token);
                let service = Arc::clone(&service);
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) = handle_connection(stream, token, service).await {
                        tracing::debug!(error = %error, "tunnel broker request rejected");
                    }
                });
            }
            () = &mut shutdown => break,
        }
    }

    reaper.abort();
    service.stop_all().await;
    Ok(())
}

async fn handle_connection(
    mut stream: TcpStream,
    token: Arc<BrokerToken>,
    service: Arc<BrokerService>,
) -> Result<()> {
    stream.set_nodelay(true)?;
    timeout(AUTH_TIMEOUT, async {
        let challenge = challenge();
        write_frame(&mut stream, &challenge).await?;
        let proof: HandshakeProof = read_frame(&mut stream).await?;
        let accepted = token.verify(&challenge.nonce, &proof);
        write_frame(&mut stream, &HandshakeResult { accepted }).await?;
        if !accepted {
            tokio::time::sleep(Duration::from_millis(250)).await;
            bail!("broker authentication failed");
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow!("broker authentication timed out"))??;

    timeout(COMMAND_TIMEOUT, async {
        let request: BrokerRequest = read_frame(&mut stream).await?;
        let response = service.handle(request).await;
        write_frame(&mut stream, &response).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow!("broker command timed out"))??;
    Ok(())
}

fn spawn_reaper(service: Arc<BrokerService>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            service.reap_expired().await;
        }
    })
}

fn client_fault(error: ket_client_core::ClientError) -> BrokerResponse {
    let issue = error.issue();
    fault(issue.code, issue.message, issue.retryable)
}

fn fault(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> BrokerResponse {
    BrokerResponse::Error {
        fault: BrokerFault {
            code: code.into(),
            message: message.into(),
            retryable,
        },
    }
}

fn random_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(target_os = "windows")]
fn default_token_file() -> PathBuf {
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("Ket")
        .join("tunnel.token")
}

#[cfg(not(target_os = "windows"))]
fn default_token_file() -> PathBuf {
    PathBuf::from("/etc/ket/tunnel.token")
}

#[cfg(target_os = "windows")]
fn default_hysteria_path() -> PathBuf {
    std::env::var_os("PROGRAMFILES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Ket")
        .join("hysteria.exe")
}

#[cfg(not(target_os = "windows"))]
fn default_hysteria_path() -> PathBuf {
    PathBuf::from("/usr/libexec/ket/hysteria")
}

#[cfg(target_os = "windows")]
fn default_xray_path() -> PathBuf {
    default_windows_install_dir().join("xray.exe")
}

#[cfg(not(target_os = "windows"))]
fn default_xray_path() -> PathBuf {
    PathBuf::from("/usr/libexec/ket/xray")
}

#[cfg(target_os = "windows")]
fn default_bridge_path() -> PathBuf {
    default_windows_install_dir().join("tun2proxy.exe")
}

#[cfg(not(target_os = "windows"))]
fn default_bridge_path() -> PathBuf {
    PathBuf::from("/usr/libexec/ket/tun2proxy")
}

#[cfg(target_os = "windows")]
fn default_windows_install_dir() -> PathBuf {
    std::env::var_os("PROGRAMFILES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Ket")
}

#[cfg(target_os = "windows")]
fn default_runtime_dir() -> PathBuf {
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("Ket")
        .join("runtime")
}

#[cfg(not(target_os = "windows"))]
fn default_runtime_dir() -> PathBuf {
    PathBuf::from("/run/ket")
}

pub fn initialize_token(path: &std::path::Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("broker token path has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o750))?;
    BrokerToken::generate()
        .write_new(path)
        .context("failed to create broker token")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicBool, Ordering},
    };

    use async_trait::async_trait;
    use ket_client_core::{
        BrokerConfig, BrokerTransportAdapter, ClientError, ProbeReport, StartedTunnel, TunnelStatus,
    };
    use ket_core::SessionTransport;
    use tokio::{net::TcpListener, sync::watch};

    use super::*;

    struct MockAdapter;

    #[async_trait]
    impl TransportAdapter for MockAdapter {
        fn supports(&self, _transport: &SessionTransport) -> bool {
            true
        }

        async fn probe(&self, _transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
            unreachable!()
        }

        async fn connect(
            &self,
            _transport: &SessionTransport,
            _probe: &ProbeReport,
        ) -> Result<StartedTunnel, ClientError> {
            unreachable!()
        }
    }

    struct MockTunnel {
        stopped: Arc<AtomicBool>,
        status: watch::Receiver<TunnelStatus>,
    }

    #[async_trait]
    impl ActiveTunnel for MockTunnel {
        fn transport_id(&self) -> &str {
            "mock"
        }

        fn status(&self) -> watch::Receiver<TunnelStatus> {
            self.status.clone()
        }

        async fn stop(&self) -> Result<(), ClientError> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn expired_leases_stop_privileged_tunnels() {
        let stopped = Arc::new(AtomicBool::new(false));
        let service = BrokerService::new(vec![Arc::new(MockAdapter)], true, Duration::from_secs(6));
        let (_, status) = watch::channel(TunnelStatus::Connected);
        service.tunnels.lock().await.insert(
            "A".repeat(48),
            TunnelLease {
                tunnel: Arc::new(MockTunnel {
                    stopped: Arc::clone(&stopped),
                    status,
                }),
                expires_at: Instant::now() - Duration::from_secs(1),
            },
        );
        service.reap_expired().await;
        assert!(stopped.load(Ordering::SeqCst));
        assert!(service.tunnels.lock().await.is_empty());
    }

    #[tokio::test]
    async fn desktop_adapter_and_service_complete_authenticated_tcp_exchange() {
        let token_path = std::env::temp_dir().join(format!("ket-service-test-{}", random_id()));
        let token = Arc::new(BrokerToken::generate());
        token.write_new(&token_path).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service =
            BrokerService::new(vec![Arc::new(MockAdapter)], false, Duration::from_secs(12));
        let server_token = Arc::clone(&token);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, server_token, service).await
        });

        let config = BrokerConfig::new(address, &token_path).unwrap();
        let readiness = BrokerTransportAdapter::new(config)
            .readiness()
            .await
            .unwrap();

        assert!(!readiness.engine_available);
        server.await.unwrap().unwrap();
        fs::remove_file(token_path).unwrap();
    }
}
