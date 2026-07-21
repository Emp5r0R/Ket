use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use ket_core::{SessionTransport, TransportProtocol};
use ket_tunnel_protocol::{
    BROKER_PROTOCOL_VERSION, BrokerRequest, BrokerResponse, BrokerToken, BrokerTunnelStatus,
    HandshakeChallenge, HandshakeResult, read_frame, write_frame,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, watch},
    time::timeout,
};

use crate::{
    ActiveTunnel, ClientError, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus,
};

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
            request_timeout: Duration::from_secs(30),
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
                    tunnel_id,
                    transport.profile.id.clone(),
                    command_rx,
                    status_tx,
                ));
                Ok(StartedTunnel {
                    tunnel: Arc::new(BrokerTunnel {
                        transport_id: transport.profile.id.clone(),
                        command_tx,
                        status_rx,
                        stop_timeout: Duration::from_secs(10),
                    }),
                    handshake_latency: Duration::from_millis(handshake_latency_ms),
                })
            }
            response => Err(unexpected_response(&transport.profile.id, response)),
        }
    }
}

enum TunnelCommand {
    Stop,
}

struct BrokerTunnel {
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
            return Ok(());
        }
        self.command_tx
            .send(TunnelCommand::Stop)
            .await
            .map_err(|_| broker_error(&self.transport_id, "tunnel monitor is unavailable", true))?;
        let mut receiver = self.status_rx.clone();
        timeout(self.stop_timeout, async {
            while matches!(*receiver.borrow(), TunnelStatus::Connected) {
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .map_err(|_| broker_error(&self.transport_id, "broker shutdown timed out", true))?;
        match receiver.borrow().clone() {
            TunnelStatus::Failed(message) => Err(broker_error(&self.transport_id, message, true)),
            _ => Ok(()),
        }
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
                        status.send_replace(next.clone());
                        if !matches!(next, TunnelStatus::Connected) {
                            break;
                        }
                    }
                    Ok(response) => {
                        status.send_replace(TunnelStatus::Failed(
                            unexpected_response(&transport_id, response).issue().message,
                        ));
                        break;
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        if failures >= 3 {
                            status.send_replace(TunnelStatus::Failed(error.issue().message));
                            break;
                        }
                    }
                }
            }
            command = commands.recv() => {
                if matches!(command, Some(TunnelCommand::Stop)) {
                    let result = transact(
                        &config,
                        BrokerRequest::Stop { tunnel_id: tunnel_id.clone() },
                        &transport_id,
                    ).await;
                    match result {
                        Ok(BrokerResponse::Stopped) => {
                            status.send_replace(TunnelStatus::Stopped);
                        }
                        Ok(response) => {
                            status.send_replace(TunnelStatus::Failed(
                                unexpected_response(&transport_id, response).issue().message,
                            ));
                        }
                        Err(error) => {
                            status.send_replace(TunnelStatus::Failed(error.issue().message));
                        }
                    }
                }
                break;
            }
        }
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
}
