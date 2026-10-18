use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use ket_core::{
    ACCESS_CODE_LENGTH, SecretString, SessionManifest, SessionStatus, TransportProtocol,
    split_session_token,
};
use tokio::{
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
};

use crate::route_health::{HttpsRouteHealthProbe, RouteHealthProbe};
use crate::validation::{validate_manifest, validate_status};
use crate::{
    ActiveTunnel, ClientError, ClientIssue, ClientPhase, ClientSnapshot, ConnectOptions,
    ControlEndpoint, ControlPlane, ProbeReport, SelectionPolicy, TransportAdapter,
    TransportFallback, TransportHistory, TransportMemory, TransportSelector, TransportSummary,
    TunnelStatus,
};

/// How many fallback reasons a snapshot keeps while a reconnect loop runs.
const MAX_FALLBACKS: usize = 8;
/// How often the tunnel is asked whether the device changed networks.
const NETWORK_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// How long the engines get to redial over a new network before the route
/// is checked.
const NETWORK_SETTLE_TIME: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProbeIdentity {
    endpoint: String,
    port: u16,
    protocol: TransportProtocol,
    tls_server_name: Option<String>,
}

impl From<&ket_core::SessionTransport> for ProbeIdentity {
    fn from(transport: &ket_core::SessionTransport) -> Self {
        Self {
            endpoint: transport.profile.endpoint.clone(),
            port: transport.profile.port,
            protocol: transport.profile.protocol.clone(),
            tls_server_name: transport.profile.tls_server_name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct CachedProbe {
    identity: ProbeIdentity,
    report: ProbeReport,
}

struct RuntimeState {
    session: Option<SessionManifest>,
    renew_at_epoch_seconds: Option<u64>,
    control_failures: u8,
    tunnel: Option<Arc<dyn ActiveTunnel>>,
    connection_requested: bool,
    preferred_protocol: Option<TransportProtocol>,
    connect_options: ConnectOptions,
    history: TransportHistory,
    probes: BTreeMap<String, CachedProbe>,
    /// The network generation the current tunnel was last checked on.
    network_generation: u64,
    /// Keeps `history` per network between connections, when set.
    memory: Option<Arc<dyn TransportMemory>>,
    /// Protocols the user left out of Automatic.
    automatic_excluded: BTreeSet<TransportProtocol>,
}

pub struct KetClient {
    endpoint: ControlEndpoint,
    client_name: String,
    api: Arc<dyn ControlPlane>,
    adapters: Vec<Arc<dyn TransportAdapter>>,
    selector: TransportSelector,
    operation: Mutex<()>,
    runtime: Mutex<RuntimeState>,
    route_health: Arc<dyn RouteHealthProbe>,
    connect_cancellation: watch::Sender<u64>,
    snapshot: watch::Sender<ClientSnapshot>,
}

impl KetClient {
    pub fn new(
        endpoint: ControlEndpoint,
        client_name: impl Into<String>,
        api: Arc<dyn ControlPlane>,
        adapters: Vec<Arc<dyn TransportAdapter>>,
        policy: SelectionPolicy,
    ) -> Result<Arc<Self>, ClientError> {
        Self::new_with_route_health(
            endpoint,
            client_name,
            api,
            adapters,
            policy,
            Arc::new(HttpsRouteHealthProbe::new()?),
        )
    }

    fn new_with_route_health(
        endpoint: ControlEndpoint,
        client_name: impl Into<String>,
        api: Arc<dyn ControlPlane>,
        adapters: Vec<Arc<dyn TransportAdapter>>,
        policy: SelectionPolicy,
        route_health: Arc<dyn RouteHealthProbe>,
    ) -> Result<Arc<Self>, ClientError> {
        let client_name = client_name.into().trim().to_owned();
        if client_name.is_empty() || client_name.chars().count() > 96 {
            return Err(ClientError::InvalidInput(
                "client name must contain between 1 and 96 characters".to_owned(),
            ));
        }
        let (snapshot, _) = watch::channel(ClientSnapshot {
            updated_at_epoch_seconds: unix_time(),
            ..ClientSnapshot::default()
        });
        let (connect_cancellation, _) = watch::channel(0);
        Ok(Arc::new(Self {
            endpoint,
            client_name,
            api,
            adapters,
            selector: TransportSelector::new(policy),
            operation: Mutex::new(()),
            runtime: Mutex::new(RuntimeState {
                session: None,
                renew_at_epoch_seconds: None,
                control_failures: 0,
                tunnel: None,
                connection_requested: false,
                preferred_protocol: None,
                connect_options: ConnectOptions::default(),
                history: TransportHistory::default(),
                probes: BTreeMap::new(),
                network_generation: 0,
                memory: None,
                automatic_excluded: BTreeSet::new(),
            }),
            route_health,
            connect_cancellation,
            snapshot,
        }))
    }

    pub fn subscribe(&self) -> watch::Receiver<ClientSnapshot> {
        self.snapshot.subscribe()
    }

    pub fn snapshot(&self) -> ClientSnapshot {
        self.snapshot.borrow().clone()
    }

    pub async fn enroll(
        &self,
        access_code: impl Into<SecretString>,
    ) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        let access_code = access_code.into();
        validate_access_code(&access_code)?;
        if self.runtime.lock().await.session.is_some() {
            return Err(ClientError::InvalidInput(
                "disconnect the current enrollment before entering a new code".to_owned(),
            ));
        }
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Enrolling;
            snapshot.issue = None;
            snapshot.reconnect_attempt = 0;
        });
        let manifest = match self
            .api
            .create_session(&self.endpoint, &access_code, &self.client_name)
            .await
        {
            Ok(manifest) => manifest,
            Err(error) => {
                self.fail(&error);
                return Err(error);
            }
        };
        if let Err(error) = validate_manifest(&manifest, unix_time()) {
            if split_session_token(manifest.session_token.expose_secret()).is_ok() {
                let _ = self
                    .api
                    .release_session(&self.endpoint, &manifest.session_token)
                    .await;
            }
            self.fail(&error);
            return Err(error);
        }

        let node = manifest.node.clone();
        let advice = manifest.updates.clone();
        let expires_at = manifest.session_expires_at_epoch_seconds;
        let access_expires_at = manifest.access_expires_at_epoch_seconds;
        let available_transports = manifest
            .transports
            .iter()
            .filter(|transport| {
                self.adapters
                    .iter()
                    .any(|adapter| adapter.supports(transport))
            })
            .map(|transport| TransportSummary::from(&transport.profile))
            .collect();
        let now = unix_time();
        let mut runtime = self.runtime.lock().await;
        runtime.renew_at_epoch_seconds = Some(renewal_deadline(now, expires_at));
        runtime.session = Some(manifest);
        drop(runtime);
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Enrolled;
            snapshot.node = Some(node);
            snapshot.available_transports = available_transports;
            snapshot.session_expires_at_epoch_seconds = Some(expires_at);
            snapshot.access_expires_at_epoch_seconds = access_expires_at;
            snapshot.issue = None;
            snapshot.apply_advice(advice);
        });
        Ok(self.snapshot())
    }

    pub async fn connect(&self) -> Result<ClientSnapshot, ClientError> {
        self.connect_with_protocol(None).await
    }

    /// Settings for every later connection, including automatic reconnects.
    pub async fn set_connect_options(&self, options: ConnectOptions) {
        self.runtime.lock().await.connect_options = options;
    }

    /// Remembers which transports worked on each network, so Automatic
    /// starts with the last one that worked where the device is now.
    pub async fn set_transport_memory(&self, memory: Arc<dyn TransportMemory>) {
        self.runtime.lock().await.memory = Some(memory);
    }

    /// Protocols Automatic never tries, unless that would leave none. An
    /// explicitly chosen protocol is always tried.
    pub async fn set_automatic_exclusions(&self, excluded: BTreeSet<TransportProtocol>) {
        self.runtime.lock().await.automatic_excluded = excluded;
    }

    pub async fn connect_with_protocol(
        &self,
        preferred_protocol: Option<TransportProtocol>,
    ) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.session.is_none() {
                return Err(ClientError::NotEnrolled);
            }
            runtime.connection_requested = true;
            runtime.preferred_protocol = preferred_protocol.clone();
        }
        self.update_snapshot(|snapshot| snapshot.preferred_protocol = preferred_protocol);
        if let Err(error) = self.renew_before_connect_locked().await {
            self.runtime.lock().await.connection_requested = false;
            return Err(error);
        }
        let result = self.connect_locked(false).await;
        if result.is_err() {
            self.runtime.lock().await.connection_requested = false;
        }
        result
    }

    pub async fn refresh(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        self.refresh_locked(false, true).await
    }

    pub async fn renew(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        self.refresh_locked(true, true).await
    }

    pub async fn maintain_once(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        let (tunnel_status, connection_requested) = {
            let runtime = self.runtime.lock().await;
            (
                runtime.tunnel.as_ref().map(|tunnel| {
                    (
                        tunnel.transport_id().to_owned(),
                        tunnel.status().borrow().clone(),
                        Arc::clone(tunnel),
                    )
                }),
                runtime.connection_requested,
            )
        };
        if let Some((transport_id, status, tunnel)) = &tunnel_status
            && !matches!(status, TunnelStatus::Connected)
        {
            self.runtime.lock().await.tunnel = None;
            self.stop_and_retain_on_failure(Arc::clone(tunnel)).await?;
            if connection_requested {
                let reason = match status {
                    TunnelStatus::Failed(message) => message.clone(),
                    _ => "the connection stopped".to_owned(),
                };
                self.record_lost_tunnel(transport_id, "transport_lost", reason)
                    .await;
                return self.reconnect_locked().await;
            }
            let enrolled = self.runtime.lock().await.session.is_some();
            self.update_snapshot(|snapshot| {
                snapshot.phase = if enrolled {
                    ClientPhase::Enrolled
                } else {
                    ClientPhase::Disconnected
                };
                snapshot.active_transport = None;
                snapshot.connected_at_epoch_seconds = None;
                snapshot.handshake_latency_ms = None;
                snapshot.reconnect_attempt = 0;
                snapshot.issue = None;
            });
            if !enrolled {
                return Ok(self.snapshot());
            }
        }
        if tunnel_status.is_none() && connection_requested {
            return self.reconnect_locked().await;
        }
        let renew = self
            .runtime
            .lock()
            .await
            .renew_at_epoch_seconds
            .is_some_and(|deadline| unix_time() >= deadline);
        match self.refresh_locked(renew, false).await {
            Ok(snapshot) => Ok(snapshot),
            Err(error @ ClientError::Network(_)) => {
                let (route_is_active, should_verify_route, offline) = {
                    let mut runtime = self.runtime.lock().await;
                    runtime.control_failures = runtime.control_failures.saturating_add(1);
                    let route_is_active = runtime.connection_requested && runtime.tunnel.is_some();
                    (
                        route_is_active,
                        runtime.control_failures >= 2
                            && runtime.connection_requested
                            && runtime.tunnel.is_some(),
                        runtime
                            .tunnel
                            .as_ref()
                            .is_some_and(|tunnel| tunnel.network().offline),
                    )
                };
                // With no network at all a new tunnel could not do better, and
                // tearing this one down would let traffic out unprotected the
                // moment a network returns.
                if route_is_active && offline {
                    self.update_snapshot(|snapshot| snapshot.issue = Some(error.issue()));
                    return Ok(self.snapshot());
                }
                if route_is_active && (!should_verify_route || self.route_health.is_healthy().await)
                {
                    self.update_snapshot(|snapshot| snapshot.issue = None);
                    return Ok(self.snapshot());
                }
                if !should_verify_route {
                    return Err(error);
                }
                let recovery = {
                    let mut runtime = self.runtime.lock().await;
                    if runtime.connection_requested {
                        runtime.control_failures = 0;
                        runtime.tunnel.take().map(|tunnel| {
                            let transport_id = tunnel.transport_id().to_owned();
                            (transport_id, tunnel)
                        })
                    } else {
                        None
                    }
                };
                let Some((transport_id, tunnel)) = recovery else {
                    return Err(error);
                };
                self.stop_and_retain_on_failure(tunnel).await?;
                self.record_lost_tunnel(
                    &transport_id,
                    "route_unhealthy",
                    "the protected route stopped reaching the server".to_owned(),
                )
                .await;
                self.reconnect_locked().await
            }
            Err(error) => Err(error),
        }
    }

    pub async fn disconnect(&self) -> Result<ClientSnapshot, ClientError> {
        self.cancel_connection_attempt();
        let _operation = self.operation.lock().await;
        let (tunnel, token) = {
            let mut runtime = self.runtime.lock().await;
            runtime.connection_requested = false;
            runtime.preferred_protocol = None;
            (
                runtime.tunnel.take(),
                runtime
                    .session
                    .as_ref()
                    .map(|session| session.session_token.clone()),
            )
        };
        if tunnel.is_none() && token.is_none() {
            self.update_snapshot(|snapshot| {
                snapshot.phase = ClientPhase::Disconnected;
                snapshot.node = None;
                snapshot.active_transport = None;
                snapshot.available_transports.clear();
                snapshot.preferred_protocol = None;
                snapshot.traffic = None;
                snapshot.session_expires_at_epoch_seconds = None;
                snapshot.access_expires_at_epoch_seconds = None;
                snapshot.issue = None;
            });
            return Ok(self.snapshot());
        }
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Disconnecting;
            snapshot.issue = None;
        });

        if let Some(tunnel) = tunnel
            && let Err(error) = self.stop_and_retain_on_failure(tunnel).await
        {
            self.update_snapshot(|snapshot| {
                snapshot.phase = ClientPhase::Error;
                snapshot.issue = Some(error.issue());
            });
            return Err(error);
        }
        self.update_snapshot(|snapshot| {
            snapshot.active_transport = None;
            snapshot.connected_at_epoch_seconds = None;
            snapshot.handshake_latency_ms = None;
            snapshot.reconnect_attempt = 0;
        });
        let release_error = match token {
            Some(token) => self
                .api
                .release_session(&self.endpoint, &token)
                .await
                .err()
                .filter(|error| !error.is_unauthorized()),
            None => None,
        };
        if let Some(error) = release_error {
            self.update_snapshot(|snapshot| {
                snapshot.phase = ClientPhase::Error;
                snapshot.issue = Some(error.issue());
            });
            return Err(error);
        }
        {
            let mut runtime = self.runtime.lock().await;
            runtime.session = None;
            runtime.renew_at_epoch_seconds = None;
            runtime.control_failures = 0;
            runtime.probes.clear();
        }
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Disconnected;
            snapshot.node = None;
            snapshot.active_transport = None;
            snapshot.available_transports.clear();
            snapshot.preferred_protocol = None;
            snapshot.traffic = None;
            snapshot.session_expires_at_epoch_seconds = None;
            snapshot.access_expires_at_epoch_seconds = None;
            snapshot.connected_at_epoch_seconds = None;
            snapshot.handshake_latency_ms = None;
            snapshot.reconnect_attempt = 0;
            snapshot.issue = None;
        });
        Ok(self.snapshot())
    }

    /// Stops packet forwarding while retaining the in-memory server lease.
    ///
    /// Desktop power controls use this operation so a user can reconnect during
    /// the current lease without re-entering an access grant. `disconnect`
    /// remains the explicit release/forget operation.
    pub async fn stop_tunnel(&self) -> Result<ClientSnapshot, ClientError> {
        self.cancel_connection_attempt();
        let _operation = self.operation.lock().await;
        let (tunnel, enrolled) = {
            let mut runtime = self.runtime.lock().await;
            runtime.connection_requested = false;
            runtime.control_failures = 0;
            (runtime.tunnel.take(), runtime.session.is_some())
        };
        let Some(tunnel) = tunnel else {
            self.update_snapshot(|snapshot| {
                snapshot.phase = if enrolled {
                    ClientPhase::Enrolled
                } else {
                    ClientPhase::Disconnected
                };
                snapshot.active_transport = None;
                snapshot.connected_at_epoch_seconds = None;
                snapshot.handshake_latency_ms = None;
                snapshot.reconnect_attempt = 0;
                snapshot.issue = None;
            });
            return Ok(self.snapshot());
        };

        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Disconnecting;
            snapshot.issue = None;
        });
        let result = self.stop_and_retain_on_failure(tunnel).await;
        let stopped = result.is_ok();
        self.update_snapshot(|snapshot| {
            snapshot.phase = if !stopped {
                ClientPhase::Error
            } else if enrolled {
                ClientPhase::Enrolled
            } else {
                ClientPhase::Disconnected
            };
            if stopped {
                snapshot.active_transport = None;
                snapshot.connected_at_epoch_seconds = None;
                snapshot.handshake_latency_ms = None;
                snapshot.reconnect_attempt = 0;
            }
            snapshot.issue = result.as_ref().err().map(ClientError::issue);
        });
        result.map(|()| self.snapshot())
    }

    pub fn spawn_maintenance(self: &Arc<Self>, interval: Duration) -> MaintenanceTask {
        let interval = interval.max(Duration::from_secs(5));
        let client = Arc::clone(self);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut network_ticker = tokio::time::interval(NETWORK_CHECK_INTERVAL);
            network_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if client.runtime.lock().await.session.is_some() {
                            let _ = client.maintain_once().await;
                        }
                    }
                    _ = network_ticker.tick() => {
                        if client.network_moved().await {
                            tokio::time::sleep(NETWORK_SETTLE_TIME).await;
                            let _ = client.verify_after_network_change().await;
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });
        MaintenanceTask {
            shutdown: Some(shutdown_tx),
            task: Some(task),
        }
    }

    /// True once when the tunnel reports the device is on another network.
    async fn network_moved(&self) -> bool {
        let mut runtime = self.runtime.lock().await;
        let Some(report) = runtime.tunnel.as_ref().map(|tunnel| tunnel.network()) else {
            return false;
        };
        if report.offline || report.generation == runtime.network_generation {
            return false;
        }
        runtime.network_generation = report.generation;
        true
    }

    /// Checks the route once the engines have had time to redial over a new
    /// network. A healthy route needs nothing, and nothing shows; otherwise
    /// the tunnel is replaced at once instead of after repeated failures.
    pub async fn verify_after_network_change(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        let active = {
            let runtime = self.runtime.lock().await;
            runtime
                .tunnel
                .as_ref()
                .filter(|_| runtime.connection_requested)
                .map(Arc::clone)
        };
        let Some(tunnel) = active else {
            return Ok(self.snapshot());
        };
        if tunnel.network().offline || self.route_health.is_healthy().await {
            return Ok(self.snapshot());
        }
        let current = self.runtime.lock().await.tunnel.take();
        if let Some(tunnel) = current {
            self.stop_and_retain_on_failure(tunnel).await?;
        }
        // The network changed under a working transport: it is not to blame,
        // so it is tried again first rather than cooled down.
        self.reconnect_locked().await
    }

    async fn connect_locked(&self, reconnecting: bool) -> Result<ClientSnapshot, ClientError> {
        let mut cancellation = self.connect_cancellation.subscribe();
        let stale_tunnel =
            {
                let mut runtime = self.runtime.lock().await;
                if runtime.tunnel.as_ref().is_some_and(|tunnel| {
                    matches!(*tunnel.status().borrow(), TunnelStatus::Connected)
                }) {
                    return Err(ClientError::AlreadyConnected);
                }
                runtime.tunnel.take()
            };
        if let Some(tunnel) = stale_tunnel {
            self.stop_and_retain_on_failure(tunnel).await?;
        }
        let (candidates, options, missing_preference) = {
            let mut runtime = self.runtime.lock().await;
            if let Some(memory) = runtime.memory.clone() {
                runtime.history = memory.load();
            }
            let session = runtime.session.as_ref().ok_or(ClientError::NotEnrolled)?;
            let candidates = self
                .selector
                .rank_for(
                    &session.transports,
                    &self.adapters,
                    &runtime.history,
                    unix_time(),
                    runtime.preferred_protocol.as_ref(),
                    &runtime.automatic_excluded,
                )
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            let missing_preference = runtime.preferred_protocol.clone().filter(|protocol| {
                !session
                    .transports
                    .iter()
                    .any(|transport| &transport.profile.protocol == protocol)
            });
            (candidates, runtime.connect_options, missing_preference)
        };
        if !reconnecting {
            // A reconnect keeps the reason the previous tunnel was left.
            self.update_snapshot(|snapshot| snapshot.fallbacks.clear());
        }
        if candidates.is_empty() {
            let error = match missing_preference {
                Some(protocol) => {
                    ClientError::PreferredProtocolUnavailable(protocol_label(&protocol))
                }
                None => ClientError::NoCompatibleTransport,
            };
            self.fail(&error);
            return Err(error);
        }

        let mut last_error = None;
        for (index, transport) in candidates.into_iter().enumerate() {
            let adapter = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports(&transport))
                .expect("the selector admitted only supported transports");
            let summary = TransportSummary::from(&transport.profile);
            self.update_snapshot(|snapshot| {
                snapshot.phase = if reconnecting {
                    ClientPhase::Reconnecting
                } else {
                    ClientPhase::Probing
                };
                snapshot.active_transport = Some(summary.clone());
                snapshot.reconnect_attempt = if reconnecting { (index + 1) as u32 } else { 0 };
                snapshot.issue = None;
            });
            let probe = match tokio::select! {
                result = adapter.probe(&transport) => Some(result),
                changed = cancellation.changed() => {
                    debug_assert!(changed.is_ok(), "KetClient owns the cancellation sender");
                    None
                }
            } {
                None => return Ok(self.finish_cancelled_connection().await),
                Some(Ok(probe)) => probe,
                Some(Err(error)) => {
                    if let Some(probe) = self.cached_probe(&transport, &error).await {
                        probe
                    } else {
                        self.record_failure(&summary, &error).await;
                        last_error = Some(error);
                        continue;
                    }
                }
            };
            self.update_snapshot(|snapshot| {
                snapshot.phase = if reconnecting {
                    ClientPhase::Reconnecting
                } else {
                    ClientPhase::Connecting
                };
            });
            let connected = tokio::select! {
                result = adapter.connect(&transport, &probe, &options) => Some(result),
                changed = cancellation.changed() => {
                    debug_assert!(changed.is_ok(), "KetClient owns the cancellation sender");
                    None
                }
            };
            match connected {
                None => return Ok(self.finish_cancelled_connection().await),
                Some(Ok(started)) => {
                    if cancellation.has_changed().unwrap_or(false) {
                        if let Err(error) = self.stop_and_retain_on_failure(started.tunnel).await {
                            self.fail(&error);
                            return Err(error);
                        }
                        return Ok(self.finish_cancelled_connection().await);
                    }
                    let latency =
                        started.handshake_latency.as_millis().min(u64::MAX as u128) as u64;
                    self.cache_probe(&transport, &probe).await;
                    {
                        let mut runtime = self.runtime.lock().await;
                        runtime
                            .history
                            .record_success(&transport.profile.id, latency);
                        if let Some(memory) = &runtime.memory {
                            memory.save(&runtime.history);
                        }
                        runtime.network_generation = started.tunnel.network().generation;
                        runtime.tunnel = Some(started.tunnel);
                    }
                    self.update_snapshot(|snapshot| {
                        snapshot.phase = ClientPhase::Connected;
                        snapshot.active_transport = Some(summary);
                        snapshot.handshake_latency_ms = Some(latency);
                        snapshot.connected_at_epoch_seconds = Some(unix_time());
                        snapshot.reconnect_attempt = 0;
                        snapshot.issue = None;
                    });
                    return Ok(self.snapshot());
                }
                Some(Err(error)) => {
                    self.record_failure(&summary, &error).await;
                    last_error = Some(error);
                }
            }
        }
        let error = last_error.unwrap_or(ClientError::NoCompatibleTransport);
        self.fail(&error);
        Err(error)
    }

    fn cancel_connection_attempt(&self) {
        self.connect_cancellation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    async fn finish_cancelled_connection(&self) -> ClientSnapshot {
        self.runtime.lock().await.connection_requested = false;
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Enrolled;
            snapshot.active_transport = None;
            snapshot.connected_at_epoch_seconds = None;
            snapshot.handshake_latency_ms = None;
            snapshot.reconnect_attempt = 0;
            snapshot.issue = None;
        });
        self.snapshot()
    }

    async fn reconnect_locked(&self) -> Result<ClientSnapshot, ClientError> {
        match self.connect_locked(true).await {
            Ok(snapshot) => Ok(snapshot),
            Err(connection_error) => {
                let renew = self
                    .runtime
                    .lock()
                    .await
                    .renew_at_epoch_seconds
                    .is_some_and(|deadline| unix_time() >= deadline);
                let refresh = self.refresh_locked(renew, false).await;
                if let Err(error) = refresh
                    && error.ends_session()
                {
                    return Err(error);
                }
                self.fail(&connection_error);
                Err(connection_error)
            }
        }
    }

    async fn renew_before_connect_locked(&self) -> Result<(), ClientError> {
        let now = unix_time();
        let (renew_due, expires_at) = {
            let runtime = self.runtime.lock().await;
            let session = runtime.session.as_ref().ok_or(ClientError::NotEnrolled)?;
            (
                runtime
                    .renew_at_epoch_seconds
                    .is_some_and(|deadline| now >= deadline),
                session.session_expires_at_epoch_seconds,
            )
        };
        if !renew_due {
            return Ok(());
        }
        match self.refresh_locked(true, false).await {
            Ok(_) => Ok(()),
            Err(ClientError::Network(_)) if unix_time() < expires_at => {
                self.update_snapshot(|snapshot| snapshot.issue = None);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn refresh_locked(
        &self,
        renew: bool,
        surface_errors: bool,
    ) -> Result<ClientSnapshot, ClientError> {
        let token = self
            .runtime
            .lock()
            .await
            .session
            .as_ref()
            .map(|session| session.session_token.clone())
            .ok_or(ClientError::NotEnrolled)?;
        let status = if renew {
            self.api.renew_session(&self.endpoint, &token).await
        } else {
            self.api.session_status(&self.endpoint, &token).await
        };
        match status {
            Ok(status) => {
                if let Err(error) = validate_status(&status, &token, &self.client_name, unix_time())
                {
                    if surface_errors {
                        self.update_snapshot(|snapshot| {
                            snapshot.issue = Some(error.issue());
                        });
                    }
                    return Err(error);
                }
                self.apply_status(status, renew).await;
                Ok(self.snapshot())
            }
            Err(error) if error.ends_session() => {
                let tunnel = {
                    let mut runtime = self.runtime.lock().await;
                    runtime.connection_requested = false;
                    runtime.preferred_protocol = None;
                    runtime.tunnel.take()
                };
                if let Some(tunnel) = tunnel
                    && let Err(cleanup_error) = self.stop_and_retain_on_failure(tunnel).await
                {
                    self.update_snapshot(|snapshot| {
                        snapshot.phase = ClientPhase::Error;
                        snapshot.issue = Some(cleanup_error.issue());
                    });
                    return Err(cleanup_error);
                }
                {
                    let mut runtime = self.runtime.lock().await;
                    runtime.session = None;
                    runtime.renew_at_epoch_seconds = None;
                    runtime.control_failures = 0;
                    runtime.probes.clear();
                }
                self.update_snapshot(|snapshot| {
                    snapshot.phase = ClientPhase::Error;
                    snapshot.active_transport = None;
                    snapshot.available_transports.clear();
                    snapshot.preferred_protocol = None;
                    snapshot.traffic = None;
                    snapshot.session_expires_at_epoch_seconds = None;
                    snapshot.access_expires_at_epoch_seconds = None;
                    snapshot.connected_at_epoch_seconds = None;
                    snapshot.handshake_latency_ms = None;
                    snapshot.reconnect_attempt = 0;
                    snapshot.issue = Some(error.issue());
                    if let Some(update) = error.required_update() {
                        snapshot.client_update = Some(update.clone());
                    }
                });
                Err(error)
            }
            Err(error) => {
                if surface_errors {
                    self.update_snapshot(|snapshot| {
                        snapshot.issue = Some(error.issue());
                    });
                }
                Err(error)
            }
        }
    }

    async fn apply_status(&self, status: SessionStatus, renewed: bool) {
        {
            let mut runtime = self.runtime.lock().await;
            if let Some(session) = &mut runtime.session {
                session.node = status.node.clone();
                session.session_expires_at_epoch_seconds = status.expires_at_epoch_seconds;
                session.access_expires_at_epoch_seconds = status.access_expires_at_epoch_seconds;
            }
            if renewed {
                runtime.renew_at_epoch_seconds = Some(renewal_deadline(
                    unix_time(),
                    status.expires_at_epoch_seconds,
                ));
            }
            runtime.control_failures = 0;
        }
        self.update_snapshot(|snapshot| {
            snapshot.node = Some(status.node);
            snapshot.traffic = Some(status.traffic);
            snapshot.session_expires_at_epoch_seconds = Some(status.expires_at_epoch_seconds);
            snapshot.access_expires_at_epoch_seconds = status.access_expires_at_epoch_seconds;
            snapshot.issue = None;
            snapshot.apply_advice(status.updates);
        });
    }

    /// Cools a transport down after it failed to connect and records why.
    async fn record_failure(&self, transport: &TransportSummary, error: &ClientError) {
        self.cool_down(&transport.id).await;
        self.note_fallback(transport.clone(), error.issue());
    }

    /// Cools down a transport whose connected tunnel was lost and records why.
    async fn record_lost_tunnel(&self, transport_id: &str, code: &str, message: String) {
        self.cool_down(transport_id).await;
        let transport = self
            .snapshot()
            .active_transport
            .filter(|transport| transport.id == transport_id);
        if let Some(transport) = transport {
            self.note_fallback(
                transport,
                ClientIssue {
                    code: code.to_owned(),
                    message,
                    retryable: true,
                },
            );
        }
    }

    async fn cool_down(&self, transport_id: &str) {
        let mut runtime = self.runtime.lock().await;
        runtime.history.record_failure(
            transport_id,
            unix_time(),
            self.selector.policy().failure_cooldown_seconds,
        );
        if let Some(memory) = &runtime.memory {
            memory.save(&runtime.history);
        }
    }

    fn note_fallback(&self, transport: TransportSummary, issue: ClientIssue) {
        self.update_snapshot(|snapshot| {
            snapshot
                .fallbacks
                .push(TransportFallback { transport, issue });
            let excess = snapshot.fallbacks.len().saturating_sub(MAX_FALLBACKS);
            snapshot.fallbacks.drain(..excess);
        });
    }

    async fn stop_and_retain_on_failure(
        &self,
        tunnel: Arc<dyn ActiveTunnel>,
    ) -> Result<(), ClientError> {
        match tunnel.stop().await {
            Ok(()) => Ok(()),
            Err(error) => {
                let mut runtime = self.runtime.lock().await;
                if runtime.tunnel.is_none() {
                    runtime.tunnel = Some(tunnel);
                }
                Err(error)
            }
        }
    }

    async fn cache_probe(&self, transport: &ket_core::SessionTransport, report: &ProbeReport) {
        self.runtime.lock().await.probes.insert(
            transport.profile.id.clone(),
            CachedProbe {
                identity: ProbeIdentity::from(transport),
                report: report.clone(),
            },
        );
    }

    async fn cached_probe(
        &self,
        transport: &ket_core::SessionTransport,
        error: &ClientError,
    ) -> Option<ProbeReport> {
        if !error.is_dns_resolution_failure() {
            return None;
        }
        let identity = ProbeIdentity::from(transport);
        self.runtime
            .lock()
            .await
            .probes
            .get(&transport.profile.id)
            .filter(|cached| cached.identity == identity)
            .map(|cached| cached.report.clone())
    }

    fn fail(&self, error: &ClientError) {
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Error;
            snapshot.issue = Some(error.issue());
            if let Some(update) = error.required_update() {
                snapshot.client_update = Some(update.clone());
            }
        });
    }

    fn update_snapshot(&self, update: impl FnOnce(&mut ClientSnapshot)) {
        let mut snapshot = self.snapshot.borrow().clone();
        update(&mut snapshot);
        snapshot.updated_at_epoch_seconds = unix_time();
        self.snapshot.send_replace(snapshot);
    }
}

pub struct MaintenanceTask {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl MaintenanceTask {
    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for MaintenanceTask {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn validate_access_code(code: &SecretString) -> Result<(), ClientError> {
    if code.len() == ACCESS_CODE_LENGTH
        && code
            .expose_secret()
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
    {
        Ok(())
    } else {
        Err(ClientError::InvalidInput(format!(
            "access code must contain exactly {ACCESS_CODE_LENGTH} ASCII letters or digits"
        )))
    }
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// How the apps name a protocol when no server profile names it.
fn protocol_label(protocol: &TransportProtocol) -> String {
    match protocol {
        TransportProtocol::Hysteria2 => "Hysteria 2",
        TransportProtocol::Ikev2 => "IKEv2",
        TransportProtocol::OpenVpnStunnel => "OpenVPN TLS",
        TransportProtocol::Shadowsocks2022 => "Shadowsocks 2022",
        TransportProtocol::Stealth => "HTTPS Stealth",
        TransportProtocol::VlessXtlsReality => "VLESS + REALITY",
        TransportProtocol::WireGuard => "WireGuard TLS",
        TransportProtocol::XorScrambled => "Hysteria XOR",
    }
    .to_owned()
}

fn renewal_deadline(now: u64, expires_at: u64) -> u64 {
    now.saturating_add(expires_at.saturating_sub(now) / 2)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        net::{Ipv4Addr, SocketAddr},
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use ket_core::{
        ClientUpdate, ClientUpdateStatus, HealthState, Network, NodeLocation, NodeStatus,
        SessionTraffic, SessionTransport, TransportCredential, TransportProfile, TransportProtocol,
        UpdateAdvice,
    };

    use super::*;
    use crate::{ProbeReport, StartedTunnel};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn lifecycle_falls_back_updates_metrics_and_releases_the_session() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let stopped = Arc::new(AtomicBool::new(false));
        let adapter = Arc::new(MockAdapter {
            failing_ids: BTreeSet::from(["hy2-blocked".to_owned()]),
            stopped: Arc::clone(&stopped),
        });
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter],
            SelectionPolicy::default(),
        )
        .unwrap();

        let enrolled = client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        assert_eq!(enrolled.phase, ClientPhase::Enrolled);
        let connected = client.connect().await.unwrap();
        assert_eq!(connected.phase, ClientPhase::Connected);
        assert_eq!(
            connected.active_transport.as_ref().unwrap().id,
            "hy2-working"
        );
        assert_eq!(connected.handshake_latency_ms, Some(42));

        let refreshed = client.refresh().await.unwrap();
        assert_eq!(refreshed.traffic.as_ref().unwrap().bytes_received, 2048);
        let stopped_snapshot = client.stop_tunnel().await.unwrap();
        assert_eq!(stopped_snapshot.phase, ClientPhase::Enrolled);
        assert!(stopped.load(Ordering::SeqCst));
        assert_eq!(api.releases.load(Ordering::SeqCst), 0);

        let reconnected = client.connect().await.unwrap();
        assert_eq!(reconnected.phase, ClientPhase::Connected);
        let disconnected = client.disconnect().await.unwrap();
        assert_eq!(disconnected.phase, ClientPhase::Disconnected);
        assert_eq!(api.releases.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn reconnect_uses_the_last_verified_address_when_dns_is_unavailable() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        client.connect().await.unwrap();
        client.stop_tunnel().await.unwrap();
        adapter.set_dns_unavailable(true);
        let reconnected = client.connect().await.unwrap();

        assert_eq!(reconnected.phase, ClientPhase::Connected);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.probe_attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_manual_connection_does_not_enter_background_recovery() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        adapter.set_blocked(["hy2-primary"]);
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        assert!(client.connect().await.is_err());
        assert!(!client.runtime.lock().await.connection_requested);
        adapter.set_blocked([]);
        client.maintain_once().await.unwrap();

        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(api.statuses.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalid_access_codes_never_reach_the_server() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let client = KetClient::new(
            ControlEndpoint::parse("http://localhost:8787").unwrap(),
            "Test device",
            api.clone(),
            vec![],
            SelectionPolicy::default(),
        )
        .unwrap();
        assert!(client.enroll("short").await.is_err());
        assert_eq!(api.enrollments.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn explicit_protocol_selection_never_falls_back_to_another_protocol() {
        let mut manifest = test_manifest();
        manifest.transports = vec![
            test_reality_transport("reality-primary", 1),
            test_transport("hy2-primary", 2),
            test_transport("hy2-secondary", 3),
        ];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();

        let enrolled = client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        assert_eq!(enrolled.available_transports.len(), 3);

        adapter.set_blocked(["hy2-primary"]);
        let connected = client
            .connect_with_protocol(Some(TransportProtocol::Hysteria2))
            .await
            .unwrap();
        assert_eq!(
            connected.preferred_protocol,
            Some(TransportProtocol::Hysteria2)
        );
        assert_eq!(
            connected
                .active_transport
                .as_ref()
                .map(|item| item.id.as_str()),
            Some("hy2-secondary")
        );

        adapter.set_blocked(["hy2-primary", "hy2-secondary"]);
        adapter.fail("hy2-secondary");
        let error = client.maintain_once().await.unwrap_err();
        assert!(matches!(error, ClientError::Transport { .. }));
        let failed = client.snapshot();
        assert_eq!(
            failed.preferred_protocol,
            Some(TransportProtocol::Hysteria2)
        );
        assert_eq!(
            failed.active_transport.as_ref().map(|item| &item.protocol),
            Some(&TransportProtocol::Hysteria2)
        );
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn direct_reconnect_stops_a_failed_handle_before_starting_a_replacement() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        client.connect().await.unwrap();
        adapter.fail("hy2-primary");
        let reconnected = client.connect().await.unwrap();

        assert_eq!(reconnected.phase, ClientPhase::Connected);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_user_stop_cleanup_is_retried_without_reconnecting() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.connect().await.unwrap();
        adapter.set_stop_failure(true);

        let stop_error = client.stop_tunnel().await.unwrap_err();
        assert!(matches!(stop_error, ClientError::Transport { .. }));
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);

        let retry_error = client.maintain_once().await.unwrap_err();
        assert!(matches!(retry_error, ClientError::Transport { .. }));
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);

        adapter.set_stop_failure(false);
        let maintained = client.maintain_once().await.unwrap();
        assert_eq!(maintained.phase, ClientPhase::Enrolled);
        assert_eq!(maintained.active_transport, None);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn authorization_loss_waits_for_tunnel_cleanup_before_dropping_ownership() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.connect().await.unwrap();
        adapter.set_stop_failure(true);
        api.set_unauthorized(true);

        let cleanup_error = client.maintain_once().await.unwrap_err();
        assert!(matches!(cleanup_error, ClientError::Transport { .. }));
        assert!(client.runtime.lock().await.session.is_some());
        assert!(client.runtime.lock().await.tunnel.is_some());
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);

        adapter.set_stop_failure(false);
        let authorization_error = client.maintain_once().await.unwrap_err();
        assert!(authorization_error.is_unauthorized());
        assert!(client.runtime.lock().await.session.is_none());
        assert!(client.runtime.lock().await.tunnel.is_none());
        assert_eq!(client.snapshot().issue.unwrap().code, "unauthorized");
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
    }

    fn test_update(status: ClientUpdateStatus) -> ClientUpdate {
        ClientUpdate {
            status,
            latest_version: "9.0.0".to_owned(),
            required_after_epoch_seconds: None,
            summary: Some("Fixes".to_owned()),
            download_url: "https://example.test/ket".to_owned(),
        }
    }

    #[tokio::test]
    async fn update_advice_is_shown_and_a_required_update_ends_the_session() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        manifest.updates = Some(UpdateAdvice {
            server_version: "9.0.0".to_owned(),
            client: test_update(ClientUpdateStatus::Optional),
        });
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        let enrolled = client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        assert_eq!(enrolled.server_version.as_deref(), Some("9.0.0"));
        assert_eq!(
            enrolled.client_update.unwrap().status,
            ClientUpdateStatus::Optional
        );
        client.connect().await.unwrap();

        // A status response from an older server keeps the last advice.
        client.refresh().await.unwrap();
        assert!(client.snapshot().client_update.is_some());

        api.set_update_required(true);
        let error = client.maintain_once().await.unwrap_err();
        assert!(error.required_update().is_some());
        assert!(client.runtime.lock().await.session.is_none());
        assert!(client.runtime.lock().await.tunnel.is_none());
        let snapshot = client.snapshot();
        assert_eq!(snapshot.phase, ClientPhase::Error);
        assert_eq!(snapshot.issue.unwrap().code, "client_update_required");
        assert_eq!(
            snapshot.client_update.unwrap().status,
            ClientUpdateStatus::Required
        );
    }

    #[tokio::test]
    async fn stop_interrupts_an_inflight_transport_probe() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("blocking-probe", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(BlockingAdapter {
            probe_started: Notify::new(),
        });
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        let connecting_client = Arc::clone(&client);
        let connecting = tokio::spawn(async move { connecting_client.connect().await });
        adapter.probe_started.notified().await;

        let stopped = tokio::time::timeout(Duration::from_secs(1), client.stop_tunnel())
            .await
            .expect("stop should interrupt a blocked probe")
            .unwrap();
        let cancelled = tokio::time::timeout(Duration::from_secs(1), connecting)
            .await
            .expect("connect task should finish after cancellation")
            .expect("connect task should not panic")
            .unwrap();

        assert_eq!(stopped.phase, ClientPhase::Enrolled);
        assert_eq!(cancelled.phase, ClientPhase::Enrolled);
        assert_eq!(client.snapshot().active_transport, None);
        assert_eq!(client.snapshot().issue, None);
    }

    #[tokio::test]
    async fn malformed_status_preserves_the_last_known_good_snapshot() {
        let mut malformed_status = test_status();
        malformed_status.node.location.longitude = 181.0;
        let api = Arc::new(MockApi::with_status(test_manifest(), malformed_status));
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![],
            SelectionPolicy::default(),
        )
        .unwrap();

        let enrolled = client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        let error = client.refresh().await.unwrap_err();
        let current = client.snapshot();

        assert!(matches!(error, ClientError::InvalidResponse(_)));
        assert_eq!(current.phase, ClientPhase::Enrolled);
        assert_eq!(current.node, enrolled.node);
        assert_eq!(current.traffic, None);
        assert_eq!(
            current.issue.as_ref().map(|issue| issue.code.as_str()),
            Some("invalid_server_response")
        );
    }

    #[tokio::test]
    async fn maintenance_renews_at_the_lease_midpoint_and_reschedules() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.runtime.lock().await.renew_at_epoch_seconds = Some(unix_time());

        client.maintain_once().await.unwrap();
        assert_eq!(api.renewals.load(Ordering::SeqCst), 1);
        assert_eq!(api.statuses.load(Ordering::SeqCst), 0);

        client.maintain_once().await.unwrap();
        assert_eq!(api.renewals.load(Ordering::SeqCst), 1);
        assert_eq!(api.statuses.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn connect_renews_a_due_lease_before_starting_the_transport() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.runtime.lock().await.renew_at_epoch_seconds = Some(unix_time());

        client.connect().await.unwrap();

        assert_eq!(api.renewals.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn connect_can_use_an_unexpired_cached_lease_during_a_control_timeout() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.runtime.lock().await.renew_at_epoch_seconds = Some(unix_time());
        api.fail_next_control_requests(1);

        let connected = client.connect().await.unwrap();

        assert_eq!(connected.phase, ClientPhase::Connected);
        assert_eq!(connected.issue, None);
        assert_eq!(api.renewals.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn connect_does_not_start_transport_with_an_expired_unrenewed_lease() {
        let mut manifest = test_manifest();
        manifest.transports = vec![test_transport("hy2-primary", 1)];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        {
            let mut runtime = client.runtime.lock().await;
            runtime.renew_at_epoch_seconds = Some(unix_time());
            runtime
                .session
                .as_mut()
                .expect("client remains enrolled")
                .session_expires_at_epoch_seconds = unix_time();
        }
        api.fail_next_control_requests(1);

        let error = client.connect().await.unwrap_err();

        assert!(matches!(error, ClientError::Network(_)));
        assert_eq!(api.renewals.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 0);
        assert!(!client.runtime.lock().await.connection_requested);
    }

    #[tokio::test]
    async fn maintenance_recovers_after_control_timeouts_when_the_route_is_unhealthy() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let adapter = Arc::new(RecoveryAdapter::new());
        let route_health = Arc::new(MockRouteHealth::new(false));
        let client = KetClient::new_with_route_health(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
            route_health.clone(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.connect().await.unwrap();
        api.fail_next_control_requests(2);

        let retained = client.maintain_once().await.unwrap();
        assert_eq!(retained.phase, ClientPhase::Connected);
        assert_eq!(retained.issue, None);
        let recovered = client.maintain_once().await.unwrap();

        assert_eq!(recovered.phase, ClientPhase::Connected);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
        assert_eq!(route_health.probes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn maintenance_preserves_a_healthy_route_during_control_timeouts() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let adapter = Arc::new(RecoveryAdapter::new());
        let route_health = Arc::new(MockRouteHealth::new(true));
        let client = KetClient::new_with_route_health(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
            route_health.clone(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.connect().await.unwrap();
        api.fail_next_control_requests(3);

        for _ in 0..3 {
            let retained = client.maintain_once().await.unwrap();
            assert_eq!(retained.phase, ClientPhase::Connected);
            assert_eq!(retained.issue, None);
        }

        assert_eq!(client.snapshot().phase, ClientPhase::Connected);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 0);
        assert_eq!(route_health.probes.load(Ordering::SeqCst), 2);
    }

    async fn connected_recovery_client(
        healthy_route: bool,
    ) -> (
        Arc<KetClient>,
        Arc<MockApi>,
        Arc<RecoveryAdapter>,
        Arc<MockRouteHealth>,
    ) {
        let api = Arc::new(MockApi::new(test_manifest()));
        let adapter = Arc::new(RecoveryAdapter::new());
        let route_health = Arc::new(MockRouteHealth::new(healthy_route));
        let client = KetClient::new_with_route_health(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy::default(),
            route_health.clone(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        client.connect().await.unwrap();
        (client, api, adapter, route_health)
    }

    #[tokio::test]
    async fn without_any_network_the_tunnel_is_held_instead_of_torn_down() {
        let (client, api, adapter, route_health) = connected_recovery_client(false).await;
        adapter.report_network(0, true);
        api.fail_next_control_requests(4);

        for _ in 0..4 {
            let held = client.maintain_once().await.unwrap();
            assert_eq!(held.phase, ClientPhase::Connected);
            assert_eq!(
                held.issue.map(|issue| issue.code).as_deref(),
                Some("network_unavailable")
            );
        }
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 0);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(route_health.probes.load(Ordering::SeqCst), 0);
        assert!(!client.network_moved().await, "an outage is not a move");
    }

    #[tokio::test]
    async fn a_new_network_with_a_working_route_changes_nothing() {
        let (client, _api, adapter, route_health) = connected_recovery_client(true).await;
        assert!(!client.network_moved().await);
        adapter.report_network(1, false);
        assert!(client.network_moved().await);
        assert!(!client.network_moved().await, "each move is reported once");

        let snapshot = client.verify_after_network_change().await.unwrap();
        assert_eq!(snapshot.phase, ClientPhase::Connected);
        assert_eq!(route_health.probes.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 0);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_new_network_that_broke_the_route_replaces_the_tunnel_at_once() {
        let (client, _api, adapter, route_health) = connected_recovery_client(false).await;
        adapter.report_network(1, false);
        assert!(client.network_moved().await);

        let snapshot = client.verify_after_network_change().await.unwrap();
        assert_eq!(snapshot.phase, ClientPhase::Connected);
        assert_eq!(route_health.probes.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.connection_attempts.load(Ordering::SeqCst), 2);
        assert!(
            snapshot.fallbacks.is_empty(),
            "the network, not the transport, was to blame"
        );
    }

    #[derive(Default)]
    struct FakeMemory {
        history: StdMutex<TransportHistory>,
        saves: AtomicUsize,
    }

    impl TransportMemory for FakeMemory {
        fn load(&self) -> TransportHistory {
            self.history.lock().unwrap().clone()
        }
        fn save(&self, history: &TransportHistory) {
            *self.history.lock().unwrap() = history.clone();
            self.saves.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn automatic_starts_with_what_worked_on_this_network_and_remembers_it() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new_with_route_health(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
            Arc::new(MockRouteHealth::new(true)),
        )
        .unwrap();
        let memory = Arc::new(FakeMemory::default());
        memory
            .history
            .lock()
            .unwrap()
            .record_success("hy2-working", 30);
        client.set_transport_memory(memory.clone()).await;
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        let connected = client.connect().await.unwrap();
        assert_eq!(
            connected
                .active_transport
                .map(|transport| transport.id)
                .as_deref(),
            Some("hy2-working"),
            "the transport that worked here goes before a higher-priority one"
        );
        assert_eq!(adapter.probe_attempts.load(Ordering::SeqCst), 1);
        assert!(memory.saves.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn automatic_skips_unchecked_protocols() {
        let mut manifest = test_manifest();
        manifest
            .transports
            .push(test_reality_transport("reality", 3));
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new_with_route_health(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy::default(),
            Arc::new(MockRouteHealth::new(true)),
        )
        .unwrap();
        client
            .set_automatic_exclusions(BTreeSet::from([TransportProtocol::Hysteria2]))
            .await;
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        let connected = client.connect().await.unwrap();
        assert_eq!(
            connected
                .active_transport
                .map(|transport| transport.id)
                .as_deref(),
            Some("reality")
        );
    }

    #[test]
    fn renewal_deadline_uses_half_of_the_available_lease() {
        assert_eq!(renewal_deadline(100, 400), 250);
        assert_eq!(renewal_deadline(400, 400), 400);
        assert_eq!(renewal_deadline(u64::MAX - 2, u64::MAX), u64::MAX - 1);
    }

    #[tokio::test]
    async fn the_snapshot_says_why_each_transport_was_left() {
        let mut manifest = test_manifest();
        manifest.transports = vec![
            test_reality_transport("reality-primary", 1),
            test_transport("hy2-fallback", 2),
        ];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        adapter.set_blocked(["reality-primary"]);
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter.clone()],
            SelectionPolicy {
                max_attempts: 2,
                preferred_protocol: None,
                failure_cooldown_seconds: 15,
            },
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        let split = ConnectOptions {
            split_tls_hello: true,
        };
        client.set_connect_options(split).await;

        let connected = client.connect().await.unwrap();
        assert_eq!(connected.fallbacks.len(), 1);
        assert_eq!(connected.fallbacks[0].transport.id, "reality-primary");
        assert_eq!(connected.fallbacks[0].issue.code, "transport_failed");
        assert!(
            connected.fallbacks[0]
                .issue
                .message
                .contains("simulated censorship")
        );
        assert_eq!(
            *adapter.connect_options.lock().unwrap(),
            [split],
            "the chosen options reach the engine"
        );

        // A lost tunnel is a reason too, and a reconnect keeps it.
        adapter.set_blocked([]);
        adapter.fail("hy2-fallback");
        let recovered = client.maintain_once().await.unwrap();
        assert_eq!(recovered.phase, ClientPhase::Connected);
        let reasons = recovered
            .fallbacks
            .iter()
            .map(|fallback| (fallback.transport.id.as_str(), fallback.issue.code.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            [
                ("reality-primary", "transport_failed"),
                ("hy2-fallback", "transport_lost")
            ]
        );
        assert_eq!(adapter.connect_options.lock().unwrap().len(), 2);
        assert!(adapter.connect_options.lock().unwrap()[1].split_tls_hello);

        // Connecting again by hand starts a fresh account.
        client.stop_tunnel().await.unwrap();
        let fresh = client.connect().await.unwrap();
        assert!(fresh.fallbacks.is_empty());
    }

    #[tokio::test]
    async fn a_chosen_protocol_the_invite_no_longer_offers_is_named() {
        let api = Arc::new(MockApi::new(test_manifest()));
        let adapter = Arc::new(RecoveryAdapter::new());
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api,
            vec![adapter],
            SelectionPolicy::default(),
        )
        .unwrap();
        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();

        let error = client
            .connect_with_protocol(Some(TransportProtocol::Stealth))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ClientError::PreferredProtocolUnavailable(_)
        ));
        let issue = client.snapshot().issue.unwrap();
        assert_eq!(issue.code, "preferred_protocol_unavailable");
        assert_eq!(
            issue.message,
            "HTTPS Stealth is not available for this invite; choose Automatic or another protocol"
        );
    }

    #[tokio::test]
    async fn maintenance_retries_after_a_fully_blocked_recovery_round() {
        let mut manifest = test_manifest();
        manifest.transports = vec![
            test_reality_transport("reality-primary", 1),
            test_transport("hy2-fallback", 2),
        ];
        let api = Arc::new(MockApi::new(manifest));
        let adapter = Arc::new(RecoveryAdapter::new());
        adapter.set_blocked(["reality-primary"]);
        let client = KetClient::new(
            ControlEndpoint::parse("http://127.0.0.1:8787").unwrap(),
            "Linux workstation",
            api.clone(),
            vec![adapter.clone()],
            SelectionPolicy {
                max_attempts: 2,
                preferred_protocol: None,
                failure_cooldown_seconds: 15,
            },
        )
        .unwrap();

        client
            .enroll("A2345678901234567890123456789012")
            .await
            .unwrap();
        let connected = client.connect().await.unwrap();
        assert_eq!(
            connected
                .active_transport
                .as_ref()
                .map(|item| item.id.as_str()),
            Some("hy2-fallback")
        );

        adapter.set_blocked(["reality-primary", "hy2-fallback"]);
        adapter.fail("hy2-fallback");
        let error = client.maintain_once().await.unwrap_err();
        assert!(matches!(error, ClientError::Transport { .. }));
        assert_eq!(api.statuses.load(Ordering::SeqCst), 1);
        assert_eq!(client.snapshot().phase, ClientPhase::Error);

        adapter.set_blocked(["hy2-fallback"]);
        let recovered = client.maintain_once().await.unwrap();
        assert_eq!(recovered.phase, ClientPhase::Connected);
        assert_eq!(
            recovered
                .active_transport
                .as_ref()
                .map(|item| item.id.as_str()),
            Some("reality-primary")
        );

        let connection_attempts = adapter.connection_attempts.load(Ordering::SeqCst);
        let stopped = client.stop_tunnel().await.unwrap();
        assert_eq!(stopped.phase, ClientPhase::Enrolled);
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 2);

        let maintained = client.maintain_once().await.unwrap();
        assert_eq!(maintained.phase, ClientPhase::Enrolled);
        assert_eq!(
            adapter.connection_attempts.load(Ordering::SeqCst),
            connection_attempts
        );
        assert_eq!(api.statuses.load(Ordering::SeqCst), 2);

        client.disconnect().await.unwrap();
        assert_eq!(api.releases.load(Ordering::SeqCst), 1);
    }

    struct MockApi {
        manifest: SessionManifest,
        status: SessionStatus,
        enrollments: AtomicUsize,
        releases: AtomicUsize,
        statuses: AtomicUsize,
        renewals: AtomicUsize,
        control_failures_remaining: AtomicUsize,
        unauthorized: AtomicBool,
        update_required: AtomicBool,
    }

    impl MockApi {
        fn new(manifest: SessionManifest) -> Self {
            Self::with_status(manifest, test_status())
        }

        fn with_status(manifest: SessionManifest, status: SessionStatus) -> Self {
            Self {
                manifest,
                status,
                enrollments: AtomicUsize::new(0),
                releases: AtomicUsize::new(0),
                statuses: AtomicUsize::new(0),
                renewals: AtomicUsize::new(0),
                control_failures_remaining: AtomicUsize::new(0),
                unauthorized: AtomicBool::new(false),
                update_required: AtomicBool::new(false),
            }
        }

        fn set_update_required(&self, required: bool) {
            self.update_required.store(required, Ordering::SeqCst);
        }

        fn refusal(&self) -> Option<ClientError> {
            if self.unauthorized.load(Ordering::SeqCst) {
                return Some(ClientError::Api {
                    status: 401,
                    code: "unauthorized".to_owned(),
                    message: "session credentials are invalid".to_owned(),
                });
            }
            self.update_required.load(Ordering::SeqCst).then(|| {
                ClientError::UpdateRequired(Box::new(test_update(ClientUpdateStatus::Required)))
            })
        }

        fn fail_next_control_requests(&self, count: usize) {
            self.control_failures_remaining
                .store(count, Ordering::SeqCst);
        }

        fn set_unauthorized(&self, unauthorized: bool) {
            self.unauthorized.store(unauthorized, Ordering::SeqCst);
        }

        fn control_request_fails(&self) -> bool {
            self.control_failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    (remaining > 0).then(|| remaining - 1)
                })
                .is_ok()
        }
    }

    #[async_trait]
    impl ControlPlane for MockApi {
        async fn create_session(
            &self,
            _endpoint: &ControlEndpoint,
            _access_code: &SecretString,
            _client_name: &str,
        ) -> Result<SessionManifest, ClientError> {
            self.enrollments.fetch_add(1, Ordering::SeqCst);
            Ok(self.manifest.clone())
        }

        async fn session_status(
            &self,
            _endpoint: &ControlEndpoint,
            _token: &SecretString,
        ) -> Result<SessionStatus, ClientError> {
            self.statuses.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.refusal() {
                return Err(error);
            }
            if self.control_request_fails() {
                return Err(ClientError::Network("request timed out"));
            }
            Ok(self.status.clone())
        }

        async fn renew_session(
            &self,
            _endpoint: &ControlEndpoint,
            _token: &SecretString,
        ) -> Result<SessionStatus, ClientError> {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.refusal() {
                return Err(error);
            }
            if self.control_request_fails() {
                return Err(ClientError::Network("request timed out"));
            }
            Ok(self.status.clone())
        }

        async fn release_session(
            &self,
            _endpoint: &ControlEndpoint,
            _token: &SecretString,
        ) -> Result<(), ClientError> {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct MockAdapter {
        failing_ids: BTreeSet<String>,
        stopped: Arc<AtomicBool>,
    }

    struct RecoveryAdapter {
        blocked: StdMutex<BTreeSet<String>>,
        dns_unavailable: AtomicBool,
        tunnels: StdMutex<BTreeMap<String, watch::Sender<TunnelStatus>>>,
        probe_attempts: AtomicUsize,
        connection_attempts: AtomicUsize,
        connect_options: StdMutex<Vec<ConnectOptions>>,
        stops: Arc<AtomicUsize>,
        stop_failure: Arc<AtomicBool>,
        /// What every tunnel from this adapter reports about the network.
        network: Arc<StdMutex<crate::NetworkReport>>,
    }

    struct BlockingAdapter {
        probe_started: Notify,
    }

    struct MockRouteHealth {
        healthy: AtomicBool,
        probes: AtomicUsize,
    }

    impl MockRouteHealth {
        fn new(healthy: bool) -> Self {
            Self {
                healthy: AtomicBool::new(healthy),
                probes: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl RouteHealthProbe for MockRouteHealth {
        async fn is_healthy(&self) -> bool {
            self.probes.fetch_add(1, Ordering::SeqCst);
            self.healthy.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TransportAdapter for BlockingAdapter {
        fn supports(&self, transport: &SessionTransport) -> bool {
            transport.profile.protocol == TransportProtocol::Hysteria2
        }

        async fn probe(&self, _transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
            self.probe_started.notify_one();
            std::future::pending().await
        }

        async fn connect(
            &self,
            _transport: &SessionTransport,
            _probe: &ProbeReport,
            _options: &ConnectOptions,
        ) -> Result<StartedTunnel, ClientError> {
            std::future::pending().await
        }
    }

    impl RecoveryAdapter {
        fn new() -> Self {
            Self {
                blocked: StdMutex::new(BTreeSet::new()),
                dns_unavailable: AtomicBool::new(false),
                tunnels: StdMutex::new(BTreeMap::new()),
                probe_attempts: AtomicUsize::new(0),
                connection_attempts: AtomicUsize::new(0),
                connect_options: StdMutex::new(Vec::new()),
                stops: Arc::new(AtomicUsize::new(0)),
                stop_failure: Arc::new(AtomicBool::new(false)),
                network: Arc::new(StdMutex::new(crate::NetworkReport::default())),
            }
        }

        fn report_network(&self, generation: u64, offline: bool) {
            *self.network.lock().expect("network lock") = crate::NetworkReport {
                generation,
                offline,
            };
        }

        fn set_blocked<const N: usize>(&self, transport_ids: [&str; N]) {
            *self.blocked.lock().expect("blocked transport lock") =
                transport_ids.into_iter().map(str::to_owned).collect();
        }

        fn set_dns_unavailable(&self, unavailable: bool) {
            self.dns_unavailable.store(unavailable, Ordering::SeqCst);
        }

        fn set_stop_failure(&self, fail: bool) {
            self.stop_failure.store(fail, Ordering::SeqCst);
        }

        fn fail(&self, transport_id: &str) {
            self.tunnels
                .lock()
                .expect("recovery tunnel lock")
                .get(transport_id)
                .expect("transport should have an active test tunnel")
                .send_replace(TunnelStatus::Failed("simulated path loss".to_owned()));
        }
    }

    #[async_trait]
    impl TransportAdapter for RecoveryAdapter {
        fn supports(&self, transport: &SessionTransport) -> bool {
            matches!(
                transport.profile.protocol,
                TransportProtocol::Hysteria2 | TransportProtocol::VlessXtlsReality
            )
        }

        async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
            self.probe_attempts.fetch_add(1, Ordering::SeqCst);
            if self.dns_unavailable.load(Ordering::SeqCst) {
                return Err(ClientError::dns_resolution(
                    &transport.profile.id,
                    "DNS timed out",
                ));
            }
            if self
                .blocked
                .lock()
                .expect("blocked transport lock")
                .contains(&transport.profile.id)
            {
                return Err(ClientError::transport(
                    &transport.profile.id,
                    "simulated censorship",
                    true,
                ));
            }
            Ok(ProbeReport {
                resolved_addresses: vec![SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 443))],
                elapsed: Duration::from_millis(5),
            })
        }

        async fn connect(
            &self,
            transport: &SessionTransport,
            _probe: &ProbeReport,
            options: &ConnectOptions,
        ) -> Result<StartedTunnel, ClientError> {
            self.connection_attempts.fetch_add(1, Ordering::SeqCst);
            self.connect_options
                .lock()
                .expect("connect options lock")
                .push(*options);
            let (status, _) = watch::channel(TunnelStatus::Connected);
            self.tunnels
                .lock()
                .expect("recovery tunnel lock")
                .insert(transport.profile.id.clone(), status.clone());
            Ok(StartedTunnel {
                tunnel: Arc::new(RecoveryTunnel {
                    transport_id: transport.profile.id.clone(),
                    status,
                    stops: Arc::clone(&self.stops),
                    stop_failure: Arc::clone(&self.stop_failure),
                    network: Arc::clone(&self.network),
                }),
                handshake_latency: Duration::from_millis(25),
            })
        }
    }

    struct RecoveryTunnel {
        transport_id: String,
        status: watch::Sender<TunnelStatus>,
        stops: Arc<AtomicUsize>,
        stop_failure: Arc<AtomicBool>,
        network: Arc<StdMutex<crate::NetworkReport>>,
    }

    #[async_trait]
    impl ActiveTunnel for RecoveryTunnel {
        fn transport_id(&self) -> &str {
            &self.transport_id
        }

        fn status(&self) -> watch::Receiver<TunnelStatus> {
            self.status.subscribe()
        }

        fn network(&self) -> crate::NetworkReport {
            *self.network.lock().expect("network lock")
        }

        async fn stop(&self) -> Result<(), ClientError> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            if self.stop_failure.load(Ordering::SeqCst) {
                self.status
                    .send_replace(TunnelStatus::Failed("simulated cleanup failure".to_owned()));
                return Err(ClientError::transport(
                    &self.transport_id,
                    "simulated cleanup failure",
                    true,
                ));
            }
            self.status.send_replace(TunnelStatus::Stopped);
            Ok(())
        }
    }

    #[async_trait]
    impl TransportAdapter for MockAdapter {
        fn supports(&self, transport: &SessionTransport) -> bool {
            transport.profile.protocol == TransportProtocol::Hysteria2
        }

        async fn probe(&self, transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
            if self.failing_ids.contains(&transport.profile.id) {
                return Err(ClientError::transport(
                    &transport.profile.id,
                    "simulated block",
                    true,
                ));
            }
            Ok(ProbeReport {
                resolved_addresses: vec![SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 443))],
                elapsed: Duration::from_millis(5),
            })
        }

        async fn connect(
            &self,
            transport: &SessionTransport,
            _probe: &ProbeReport,
            _options: &ConnectOptions,
        ) -> Result<StartedTunnel, ClientError> {
            Ok(StartedTunnel {
                tunnel: Arc::new(MockTunnel {
                    transport_id: transport.profile.id.clone(),
                    stopped: Arc::clone(&self.stopped),
                }),
                handshake_latency: Duration::from_millis(42),
            })
        }
    }

    struct MockTunnel {
        transport_id: String,
        stopped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ActiveTunnel for MockTunnel {
        fn transport_id(&self) -> &str {
            &self.transport_id
        }

        fn status(&self) -> watch::Receiver<TunnelStatus> {
            let state = if self.stopped.load(Ordering::SeqCst) {
                TunnelStatus::Stopped
            } else {
                TunnelStatus::Connected
            };
            watch::channel(state).1
        }

        async fn stop(&self) -> Result<(), ClientError> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn test_manifest() -> SessionManifest {
        SessionManifest {
            session_token: SecretString::from("A23456789012B3456789012345678901234567890123"),
            session_expires_at_epoch_seconds: unix_time() + 300,
            access_expires_at_epoch_seconds: Some(unix_time() + 3_600),
            node: test_node(),
            transports: vec![
                test_transport("hy2-blocked", 1),
                test_transport("hy2-working", 2),
            ],
            updates: None,
        }
    }

    fn test_transport(id: &str, priority: u16) -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: id.to_owned(),
                display_name: "Hysteria 2".to_owned(),
                protocol: TransportProtocol::Hysteria2,
                endpoint: "vpn.example.test".to_owned(),
                port: 443,
                network: Network::Udp,
                priority,
                tls_server_name: Some("vpn.example.test".to_owned()),
                options: BTreeMap::new(),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("A23456789012C3456789012345678901234567890123"),
                secrets: BTreeMap::new(),
            }),
        }
    }

    fn test_reality_transport(id: &str, priority: u16) -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: id.to_owned(),
                display_name: "VLESS + REALITY".to_owned(),
                protocol: TransportProtocol::VlessXtlsReality,
                endpoint: "vpn.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority,
                tls_server_name: Some("www.example.com".to_owned()),
                options: BTreeMap::from([
                    ("encryption".to_owned(), "none".to_owned()),
                    ("fingerprint".to_owned(), "chrome".to_owned()),
                    ("flow".to_owned(), "xtls-rprx-vision".to_owned()),
                    ("transport".to_owned(), "raw".to_owned()),
                ]),
            },
            credential: Some(TransportCredential {
                auth: SecretString::from("550e8400-e29b-41d4-a716-446655440000"),
                secrets: BTreeMap::from([
                    (
                        "reality_password".to_owned(),
                        SecretString::from("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                    ),
                    (
                        "reality_short_id".to_owned(),
                        SecretString::from("0123456789abcdef"),
                    ),
                ]),
            }),
        }
    }

    fn test_status() -> SessionStatus {
        SessionStatus {
            session_id: "A23456789012".to_owned(),
            client_name: "Linux workstation".to_owned(),
            expires_at_epoch_seconds: unix_time() + 300,
            access_expires_at_epoch_seconds: Some(unix_time() + 3_600),
            node: test_node(),
            traffic: SessionTraffic {
                available: true,
                bytes_sent: 1024,
                bytes_received: 2048,
                online_connections: 1,
                observed_at_epoch_seconds: unix_time(),
            },
            updates: None,
        }
    }

    fn test_node() -> NodeStatus {
        NodeStatus {
            node_id: "test-node".to_owned(),
            display_name: "Test node".to_owned(),
            public_url: "https://ket.example.test".to_owned(),
            location: NodeLocation {
                country_code: "NL".to_owned(),
                country_name: "Netherlands".to_owned(),
                city: Some("Amsterdam".to_owned()),
                latitude: 52.3676,
                longitude: 4.9041,
            },
            health: HealthState::Healthy,
            active_sessions: 1,
            session_capacity: 10,
            capacity_percent: 10.0,
            cpu_load_percent: Some(2.0),
            memory_used_bytes: Some(1024),
            memory_total_bytes: Some(4096),
            uptime_seconds: Some(100),
            observed_at_epoch_seconds: unix_time(),
        }
    }
}
