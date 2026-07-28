use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use ket_core::{SessionTransport, TransportProtocol};
use ket_tunnel_protocol::{
    BROKER_PROTOCOL_VERSION, BrokerRequest, BrokerResponse, BrokerToken, BrokerTunnelStatus,
    HandshakeChallenge, HandshakeResult, read_frame, write_frame,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    time::timeout,
};

use crate::{
    ActiveTunnel, ClientError, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus,
};

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(50);
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct BrokerConfig {
    address: SocketAddr,
    token_file: PathBuf,
    request_timeout: Duration,
    heartbeat_interval: Duration,
}

impl BrokerConfig {
    pub fn new(address: SocketAddr, token_file: impl Into<PathBuf>) -> Result<Self, ClientError> {
        if !address.ip().is_loopback() {
            return Err(ClientError::InvalidInput(
                "tunnel broker must use a loopback address".to_owned(),
            ));
        }
        Ok(Self {
            address,
            token_file: token_file.into(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            heartbeat_interval: Duration::from_secs(3),
        })
    }

    pub fn from_env() -> Result<Self, ClientError> {
        let address = std::env::var("KET_BROKER_ADDRESS")
            .unwrap_or_else(|_| ket_tunnel_protocol::DEFAULT_BROKER_ADDRESS.to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| {
                ClientError::InvalidInput(
                    "KET_BROKER_ADDRESS must be a valid socket address".to_owned(),
                )
            })?;
        let token_file = std::env::var_os("KET_BROKER_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(default_token_file);
        Self::new(address, token_file)
    }

    #[cfg(test)]
    fn with_timing(mut self, request_timeout: Duration, heartbeat_interval: Duration) -> Self {
        self.request_timeout = request_timeout;
        self.heartbeat_interval = heartbeat_interval;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerReadiness {
    pub engine_available: bool,
}

#[derive(Clone, Debug)]
pub struct BrokerTransportAdapter {
    config: BrokerConfig,
}

impl BrokerTransportAdapter {
    pub fn new(config: BrokerConfig) -> Self {
        Self { config }
    }

    pub async fn readiness(&self) -> Result<BrokerReadiness, ClientError> {
        let response = timeout(
            Duration::from_secs(2),
            transact(&self.config, BrokerRequest::Ping, "broker"),
        )
        .await
        .map_err(|_| broker_error("broker", "privileged tunnel service timed out", true))??;
        match response {
            BrokerResponse::Pong { engine_available } => Ok(BrokerReadiness { engine_available }),
            response => Err(unexpected_response("broker", response)),
        }
    }
}

#[async_trait]
impl TransportAdapter for BrokerTransportAdapter {
    fn supports(&self, transport: &SessionTransport) -> bool {
        matches!(
            transport.profile.protocol,
            TransportProtocol::Hysteria2
                | TransportProtocol::OpenVpnStunnel
                | TransportProtocol::Shadowsocks2022
                | TransportProtocol::Stealth
                | TransportProtocol::VlessXtlsReality
                | TransportProtocol::WireGuard
                | TransportProtocol::XorScrambled
        )
    }

    async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
        match transact(
            &self.config,
            BrokerRequest::Probe {
                transport: transport.clone(),
            },
            &transport.profile.id,
        )
        .await?
        {
            BrokerResponse::Probe {
                resolved_addresses,
                elapsed_ms,
            } if !resolved_addresses.is_empty() => Ok(ProbeReport {
                resolved_addresses,
                elapsed: Duration::from_millis(elapsed_ms),
            }),
            response => Err(unexpected_response(&transport.profile.id, response)),
        }
    }

    async fn connect(
        &self,
        transport: &SessionTransport,
        probe: &ProbeReport,
    ) -> Result<StartedTunnel, ClientError> {
        match transact(
            &self.config,
            BrokerRequest::Connect {
                transport: transport.clone(),
                resolved_addresses: probe.resolved_addresses.clone(),
            },
            &transport.profile.id,
        )
        .await?
        {
            BrokerResponse::Connected {
                tunnel_id,
                handshake_latency_ms,
            } if valid_tunnel_id(&tunnel_id) => {
                let (command_tx, command_rx) = mpsc::channel(1);
                let (status_tx, status_rx) = watch::channel(TunnelStatus::Connected);
                tokio::spawn(monitor_tunnel(
                    self.config.clone(),
                    tunnel_id.clone(),
                    transport.profile.id.clone(),
                    command_rx,
                    status_tx,
                ));
                Ok(StartedTunnel {
                    tunnel: Arc::new(BrokerTunnel {
                        config: self.config.clone(),
                        tunnel_id: tunnel_id.clone(),
                        transport_id: transport.profile.id.clone(),
                        command_tx,
                        status_rx,
                        stop_timeout: DEFAULT_STOP_TIMEOUT,
                    }),
                    handshake_latency: Duration::from_millis(handshake_latency_ms),
                })
            }
            response => Err(unexpected_response(&transport.profile.id, response)),
        }
    }
}

enum TunnelCommand {
    Stop {
        completed: oneshot::Sender<Result<(), ClientError>>,
    },
}

struct BrokerTunnel {
    config: BrokerConfig,
    tunnel_id: String,
    transport_id: String,
    command_tx: mpsc::Sender<TunnelCommand>,
    status_rx: watch::Receiver<TunnelStatus>,
    stop_timeout: Duration,
}

#[async_trait]
impl ActiveTunnel for BrokerTunnel {
    fn transport_id(&self) -> &str {
        &self.transport_id
    }

    fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    async fn stop(&self) -> Result<(), ClientError> {
        if !matches!(*self.status_rx.borrow(), TunnelStatus::Connected) {
            return stop_remote(
                &self.config,
                &self.tunnel_id,
                &self.transport_id,
                self.stop_timeout,
            )
            .await;
        }
        let (completed, completion) = oneshot::channel();
        if self
            .command_tx
            .send(TunnelCommand::Stop { completed })
            .await
            .is_ok()
            && let Ok(Ok(result)) = timeout(self.stop_timeout, completion).await
        {
            return result;
        }
        stop_remote(
            &self.config,
            &self.tunnel_id,
            &self.transport_id,
            self.stop_timeout,
        )
        .await
    }
}

async fn monitor_tunnel(
    config: BrokerConfig,
    tunnel_id: String,
    transport_id: String,
    mut commands: mpsc::Receiver<TunnelCommand>,
    status: watch::Sender<TunnelStatus>,
) {
    let mut ticker = tokio::time::interval(config.heartbeat_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut failures = 0_u8;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match transact(
                    &config,
                    BrokerRequest::Heartbeat { tunnel_id: tunnel_id.clone() },
                    &transport_id,
                ).await {
                    Ok(BrokerResponse::Tunnel { status: next }) => {
                        failures = 0;
                        let next = tunnel_status(next);
                        if !matches!(next, TunnelStatus::Connected) {
                            let _ = stop_remote(
                                &config,
                                &tunnel_id,
                                &transport_id,
                                DEFAULT_STOP_TIMEOUT,
                            )
                            .await;
                            status.send_replace(next);
                            break;
                        }
                        status.send_replace(next);
                    }
                    Ok(response) => {
                        let message = unexpected_response(&transport_id, response).issue().message;
                        let _ = stop_remote(
                            &config,
                            &tunnel_id,
                            &transport_id,
                            DEFAULT_STOP_TIMEOUT,
                        )
                        .await;
                        status.send_replace(TunnelStatus::Failed(message));
                        break;
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        if failures >= 3 {
                            let message = error.issue().message;
                            let _ = stop_remote(
                                &config,
                                &tunnel_id,
                                &transport_id,
                                DEFAULT_STOP_TIMEOUT,
                            )
                            .await;
                            status.send_replace(TunnelStatus::Failed(message));
                            break;
                        }
                    }
                }
            }
            command = commands.recv() => {
                let result = stop_remote(
                    &config,
                    &tunnel_id,
                    &transport_id,
                    DEFAULT_STOP_TIMEOUT,
                )
                .await;
                match &result {
                    Ok(()) => status.send_replace(TunnelStatus::Stopped),
                    Err(error) => status.send_replace(TunnelStatus::Failed(error.issue().message)),
                };
                if let Some(TunnelCommand::Stop { completed }) = command {
                    let _ = completed.send(result);
                }
                break;
            }
        }
    }
}

async fn stop_remote(
    config: &BrokerConfig,
    tunnel_id: &str,
    transport_id: &str,
    stop_timeout: Duration,
) -> Result<(), ClientError> {
    let response = timeout(
        stop_timeout,
        transact(
            config,
            BrokerRequest::Stop {
                tunnel_id: tunnel_id.to_owned(),
            },
            transport_id,
        ),
    )
    .await
    .map_err(|_| broker_error(transport_id, "broker shutdown timed out", true))??;
    match response {
        BrokerResponse::Stopped => Ok(()),
        response => Err(unexpected_response(transport_id, response)),
    }
}

async fn transact(
    config: &BrokerConfig,
    request: BrokerRequest,
    transport_id: &str,
) -> Result<BrokerResponse, ClientError> {
    timeout(config.request_timeout, async {
        let mut stream = TcpStream::connect(config.address).await.map_err(|_| {
            broker_error(
                transport_id,
                "privileged tunnel service is unavailable",
                true,
            )
        })?;
        stream
            .set_nodelay(true)
            .map_err(|_| broker_error(transport_id, "failed to secure broker connection", true))?;
        let challenge: HandshakeChallenge = read_frame(&mut stream)
            .await
            .map_err(|_| broker_error(transport_id, "broker handshake failed", true))?;
        if challenge.version != BROKER_PROTOCOL_VERSION {
            return Err(broker_error(
                transport_id,
                "tunnel service protocol version is incompatible",
                false,
            ));
        }
        let token_file = config.token_file.clone();
        let token = tokio::task::spawn_blocking(move || BrokerToken::load(&token_file))
            .await
            .map_err(|_| broker_error(transport_id, "broker token reader stopped", false))?
            .map_err(|_| {
                broker_error(
                    transport_id,
                    "tunnel service credentials are unavailable",
                    false,
                )
            })?;
        write_frame(&mut stream, &token.prove(&challenge.nonce))
            .await
            .map_err(|_| broker_error(transport_id, "broker handshake failed", true))?;
        let authenticated: HandshakeResult = read_frame(&mut stream)
            .await
            .map_err(|_| broker_error(transport_id, "broker handshake failed", true))?;
        if !authenticated.accepted {
            return Err(broker_error(
                transport_id,
                "tunnel service authentication failed",
                false,
            ));
        }
        write_frame(&mut stream, &request)
            .await
            .map_err(|_| broker_error(transport_id, "broker request failed", true))?;
        let response: BrokerResponse = read_frame(&mut stream)
            .await
            .map_err(|_| broker_error(transport_id, "broker response failed", true))?;
        match response {
            BrokerResponse::Error { fault } => {
                Err(broker_error(transport_id, fault.message, fault.retryable))
            }
            response => Ok(response),
        }
    })
    .await
    .map_err(|_| broker_error(transport_id, "tunnel service timed out", true))?
}

fn tunnel_status(status: BrokerTunnelStatus) -> TunnelStatus {
    match status {
        BrokerTunnelStatus::Connected => TunnelStatus::Connected,
        BrokerTunnelStatus::Stopped => TunnelStatus::Stopped,
        BrokerTunnelStatus::Failed(message) => TunnelStatus::Failed(message),
    }
}

fn unexpected_response(transport_id: &str, response: BrokerResponse) -> ClientError {
    tracing::warn!(transport_id, response = ?response, "unexpected tunnel broker response");
    broker_error(
        transport_id,
        "tunnel service returned an invalid response",
        false,
    )
}

fn broker_error(transport_id: &str, message: impl Into<String>, retryable: bool) -> ClientError {
    ClientError::transport(transport_id, message, retryable)
}

fn valid_tunnel_id(value: &str) -> bool {
    value.len() == 48 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
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

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use ket_tunnel_protocol::{HandshakeProof, challenge};
    use rand::{Rng, distributions::Alphanumeric};
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn broker_configuration_rejects_non_loopback_listeners() {
        assert!(BrokerConfig::new("192.0.2.10:39731".parse().unwrap(), "token").is_err());
        assert!(BrokerConfig::new("127.0.0.1:39731".parse().unwrap(), "token").is_ok());
    }

    #[tokio::test]
    async fn readiness_uses_an_authenticated_framed_exchange() {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let token_path = std::env::temp_dir().join(format!("ket-broker-token-{suffix}"));
        let token = Arc::new(BrokerToken::generate());
        token.write_new(&token_path).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_token = Arc::clone(&token);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let challenge = challenge();
            write_frame(&mut stream, &challenge).await.unwrap();
            let proof: HandshakeProof = read_frame(&mut stream).await.unwrap();
            write_frame(
                &mut stream,
                &HandshakeResult {
                    accepted: server_token.verify(&challenge.nonce, &proof),
                },
            )
            .await
            .unwrap();
            let request: BrokerRequest = read_frame(&mut stream).await.unwrap();
            assert!(matches!(request, BrokerRequest::Ping));
            write_frame(
                &mut stream,
                &BrokerResponse::Pong {
                    engine_available: true,
                },
            )
            .await
            .unwrap();
        });
        let config = BrokerConfig::new(address, &token_path)
            .unwrap()
            .with_timing(Duration::from_secs(2), Duration::from_millis(50));
        let readiness = BrokerTransportAdapter::new(config)
            .readiness()
            .await
            .unwrap();
        assert!(readiness.engine_available);
        server.await.unwrap();
        fs::remove_file(token_path).unwrap();
    }

    #[tokio::test]
    async fn terminal_local_status_still_releases_the_privileged_lease() {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let token_path = std::env::temp_dir().join(format!("ket-broker-token-{suffix}"));
        let token = Arc::new(BrokerToken::generate());
        token.write_new(&token_path).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_token = Arc::clone(&token);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            authenticate(&mut stream, &server_token).await;
            let request: BrokerRequest = read_frame(&mut stream).await.unwrap();
            assert!(matches!(
                request,
                BrokerRequest::Stop { ref tunnel_id } if tunnel_id == &"A".repeat(48)
            ));
            write_frame(&mut stream, &BrokerResponse::Stopped)
                .await
                .unwrap();
        });
        let config = BrokerConfig::new(address, &token_path)
            .unwrap()
            .with_timing(Duration::from_secs(2), Duration::from_millis(50));
        let (command_tx, command_rx) = mpsc::channel(1);
        drop(command_rx);
        let (_, status_rx) = watch::channel(TunnelStatus::Failed("heartbeat failed".to_owned()));
        let tunnel = BrokerTunnel {
            config,
            tunnel_id: "A".repeat(48),
            transport_id: "test-transport".to_owned(),
            command_tx,
            status_rx,
            stop_timeout: Duration::from_secs(2),
        };

        tunnel.stop().await.unwrap();

        server.await.unwrap();
        fs::remove_file(token_path).unwrap();
    }

    #[tokio::test]
    async fn monitor_releases_the_privileged_lease_before_publishing_failure() {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let token_path = std::env::temp_dir().join(format!("ket-broker-token-{suffix}"));
        let token = Arc::new(BrokerToken::generate());
        token.write_new(&token_path).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_token = Arc::clone(&token);
        let server = tokio::spawn(async move {
            for request_index in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                authenticate(&mut stream, &server_token).await;
                let request: BrokerRequest = read_frame(&mut stream).await.unwrap();
                if request_index == 0 {
                    assert!(matches!(request, BrokerRequest::Heartbeat { .. }));
                    write_frame(
                        &mut stream,
                        &BrokerResponse::Pong {
                            engine_available: true,
                        },
                    )
                    .await
                    .unwrap();
                } else {
                    assert!(matches!(request, BrokerRequest::Stop { .. }));
                    write_frame(&mut stream, &BrokerResponse::Stopped)
                        .await
                        .unwrap();
                }
            }
        });
        let config = BrokerConfig::new(address, &token_path)
            .unwrap()
            .with_timing(Duration::from_secs(2), Duration::from_millis(10));
        let (commands, command_rx) = mpsc::channel(1);
        let (status_tx, mut status_rx) = watch::channel(TunnelStatus::Connected);
        let monitor = tokio::spawn(monitor_tunnel(
            config,
            "A".repeat(48),
            "test-transport".to_owned(),
            command_rx,
            status_tx,
        ));

        timeout(Duration::from_secs(2), status_rx.changed())
            .await
            .expect("monitor should publish terminal status")
            .unwrap();
        assert!(matches!(*status_rx.borrow(), TunnelStatus::Failed(_)));

        drop(commands);
        monitor.await.unwrap();
        server.await.unwrap();
        fs::remove_file(token_path).unwrap();
    }

    async fn authenticate(stream: &mut TcpStream, token: &BrokerToken) {
        let challenge = challenge();
        write_frame(&mut *stream, &challenge).await.unwrap();
        let proof: HandshakeProof = read_frame(&mut *stream).await.unwrap();
        write_frame(
            stream,
            &HandshakeResult {
                accepted: token.verify(&challenge.nonce, &proof),
            },
        )
        .await
        .unwrap();
    }
}
