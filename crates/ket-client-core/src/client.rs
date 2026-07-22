use std::{sync::Arc, time::Duration};

use ket_core::{
    ACCESS_CODE_LENGTH, SecretString, SessionManifest, SessionStatus, TransportProtocol,
    split_session_token,
};
use tokio::{
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
};

use crate::validation::{validate_manifest, validate_status};
use crate::{
    ActiveTunnel, ClientError, ClientPhase, ClientSnapshot, ControlEndpoint, ControlPlane,
    SelectionPolicy, TransportAdapter, TransportHistory, TransportSelector, TransportSummary,
    TunnelStatus,
};

struct RuntimeState {
    session: Option<SessionManifest>,
    tunnel: Option<Arc<dyn ActiveTunnel>>,
    connection_requested: bool,
    preferred_protocol: Option<TransportProtocol>,
    history: TransportHistory,
}

pub struct KetClient {
    endpoint: ControlEndpoint,
    client_name: String,
    api: Arc<dyn ControlPlane>,
    adapters: Vec<Arc<dyn TransportAdapter>>,
    selector: TransportSelector,
    operation: Mutex<()>,
    runtime: Mutex<RuntimeState>,
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
                tunnel: None,
                connection_requested: false,
                preferred_protocol: None,
                history: TransportHistory::default(),
            }),
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
        self.runtime.lock().await.session = Some(manifest);
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Enrolled;
            snapshot.node = Some(node);
            snapshot.available_transports = available_transports;
            snapshot.session_expires_at_epoch_seconds = Some(expires_at);
            snapshot.access_expires_at_epoch_seconds = access_expires_at;
            snapshot.issue = None;
        });
        Ok(self.snapshot())
    }

    pub async fn connect(&self) -> Result<ClientSnapshot, ClientError> {
        self.connect_with_protocol(None).await
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
        self.connect_locked(false).await
    }

    pub async fn refresh(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        self.refresh_locked(false).await
    }

    pub async fn renew(&self) -> Result<ClientSnapshot, ClientError> {
        let _operation = self.operation.lock().await;
        self.refresh_locked(true).await
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
                    )
                }),
                runtime.connection_requested,
            )
        };
        if let Some((transport_id, status)) = &tunnel_status
            && !matches!(status, TunnelStatus::Connected)
        {
            self.runtime.lock().await.tunnel = None;
            self.record_failure(transport_id).await;
            return self.reconnect_locked().await;
        }
        if tunnel_status.is_none() && connection_requested {
            return self.reconnect_locked().await;
        }
        let expires_at = self
            .runtime
            .lock()
            .await
            .session
            .as_ref()
            .map(|session| session.session_expires_at_epoch_seconds)
            .ok_or(ClientError::NotEnrolled)?;
        self.refresh_locked(unix_time().saturating_add(60) >= expires_at)
            .await
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

        let stop_error = match tunnel {
            Some(tunnel) => tunnel.stop().await.err(),
            None => None,
        };
        let release_error = match token {
            Some(token) => self
                .api
                .release_session(&self.endpoint, &token)
                .await
                .err()
                .filter(|error| !error.is_unauthorized()),
            None => None,
        };
        self.runtime.lock().await.session = None;
        let error = stop_error.or(release_error);
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
            snapshot.issue = error.as_ref().map(ClientError::issue);
        });
        match error {
            Some(error) => Err(error),
            None => Ok(self.snapshot()),
        }
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
        let result = tunnel.stop().await;
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
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if client.runtime.lock().await.session.is_some() {
                            let _ = client.maintain_once().await;
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

    async fn connect_locked(&self, reconnecting: bool) -> Result<ClientSnapshot, ClientError> {
        let mut cancellation = self.connect_cancellation.subscribe();
        {
            let mut runtime = self.runtime.lock().await;
            if runtime
                .tunnel
                .as_ref()
                .is_some_and(|tunnel| matches!(*tunnel.status().borrow(), TunnelStatus::Connected))
            {
                return Err(ClientError::AlreadyConnected);
            }
            runtime.tunnel = None;
        }
        let candidates = {
            let runtime = self.runtime.lock().await;
            let session = runtime.session.as_ref().ok_or(ClientError::NotEnrolled)?;
            self.selector
                .rank_with_preference(
                    &session.transports,
                    &self.adapters,
                    &runtime.history,
                    unix_time(),
                    runtime.preferred_protocol.as_ref(),
                )
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };
        if candidates.is_empty() {
            let error = ClientError::NoCompatibleTransport;
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
                    self.record_failure(&transport.profile.id).await;
                    last_error = Some(error);
                    continue;
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
                result = adapter.connect(&transport, &probe) => Some(result),
                changed = cancellation.changed() => {
                    debug_assert!(changed.is_ok(), "KetClient owns the cancellation sender");
                    None
                }
            };
            match connected {
                None => return Ok(self.finish_cancelled_connection().await),
                Some(Ok(started)) => {
                    if cancellation.has_changed().unwrap_or(false) {
                        let _ = started.tunnel.stop().await;
                        return Ok(self.finish_cancelled_connection().await);
                    }
                    let latency =
                        started.handshake_latency.as_millis().min(u64::MAX as u128) as u64;
                    {
                        let mut runtime = self.runtime.lock().await;
                        runtime
                            .history
                            .record_success(&transport.profile.id, latency);
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
                    self.record_failure(&transport.profile.id).await;
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
                let expires_at = self
                    .runtime
                    .lock()
                    .await
                    .session
                    .as_ref()
                    .map(|session| session.session_expires_at_epoch_seconds)
                    .ok_or(ClientError::NotEnrolled)?;
                let refresh = self
                    .refresh_locked(unix_time().saturating_add(60) >= expires_at)
                    .await;
                if let Err(error) = refresh
                    && error.is_unauthorized()
                {
                    return Err(error);
                }
                self.fail(&connection_error);
                Err(connection_error)
            }
        }
    }

    async fn refresh_locked(&self, renew: bool) -> Result<ClientSnapshot, ClientError> {
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
                    self.update_snapshot(|snapshot| {
                        snapshot.issue = Some(error.issue());
                    });
                    return Err(error);
                }
                self.apply_status(status).await;
                Ok(self.snapshot())
            }
            Err(error) if error.is_unauthorized() => {
                let tunnel = {
                    let mut runtime = self.runtime.lock().await;
                    runtime.connection_requested = false;
                    runtime.preferred_protocol = None;
                    runtime.tunnel.take()
                };
                if let Some(tunnel) = tunnel {
                    let _ = tunnel.stop().await;
                }
                self.runtime.lock().await.session = None;
                self.update_snapshot(|snapshot| {
                    snapshot.phase = ClientPhase::Error;
                    snapshot.active_transport = None;
                    snapshot.available_transports.clear();
                    snapshot.preferred_protocol = None;
                    snapshot.session_expires_at_epoch_seconds = None;
                    snapshot.access_expires_at_epoch_seconds = None;
                    snapshot.connected_at_epoch_seconds = None;
                    snapshot.issue = Some(error.issue());
                });
                Err(error)
            }
            Err(error) => {
                self.update_snapshot(|snapshot| {
                    snapshot.issue = Some(error.issue());
                });
                Err(error)
            }
        }
    }

    async fn apply_status(&self, status: SessionStatus) {
        {
            let mut runtime = self.runtime.lock().await;
            if let Some(session) = &mut runtime.session {
                session.node = status.node.clone();
                session.session_expires_at_epoch_seconds = status.expires_at_epoch_seconds;
                session.access_expires_at_epoch_seconds = status.access_expires_at_epoch_seconds;
            }
        }
        self.update_snapshot(|snapshot| {
            snapshot.node = Some(status.node);
            snapshot.traffic = Some(status.traffic);
            snapshot.session_expires_at_epoch_seconds = Some(status.expires_at_epoch_seconds);
            snapshot.access_expires_at_epoch_seconds = status.access_expires_at_epoch_seconds;
            snapshot.issue = None;
        });
    }

    async fn record_failure(&self, transport_id: &str) {
        self.runtime.lock().await.history.record_failure(
            transport_id,
            unix_time(),
            self.selector.policy().failure_cooldown_seconds,
        );
    }

    fn fail(&self, error: &ClientError) {
        self.update_snapshot(|snapshot| {
            snapshot.phase = ClientPhase::Error;
            snapshot.issue = Some(error.issue());
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
        HealthState, Network, NodeLocation, NodeStatus, SessionTraffic, SessionTransport,
        TransportCredential, TransportProfile, TransportProtocol,
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
    async fn runtime_protocol_preference_is_visible_and_keeps_fallback_available() {
        let mut manifest = test_manifest();
        manifest.transports = vec![
            test_reality_transport("reality-primary", 1),
            test_transport("hy2-preferred", 2),
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
        assert_eq!(enrolled.available_transports.len(), 2);

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
            Some("hy2-preferred")
        );

        client.stop_tunnel().await.unwrap();
        adapter.set_blocked(["hy2-preferred"]);
        let fallback = client
            .connect_with_protocol(Some(TransportProtocol::Hysteria2))
            .await
            .unwrap();
        assert_eq!(
            fallback
                .active_transport
                .as_ref()
                .map(|item| item.id.as_str()),
            Some("reality-primary")
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
        assert_eq!(adapter.stops.load(Ordering::SeqCst), 1);

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
            }
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
            Ok(self.status.clone())
        }

        async fn renew_session(
            &self,
            _endpoint: &ControlEndpoint,
            _token: &SecretString,
        ) -> Result<SessionStatus, ClientError> {
            self.statuses.fetch_add(1, Ordering::SeqCst);
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
        tunnels: StdMutex<BTreeMap<String, watch::Sender<TunnelStatus>>>,
        connection_attempts: AtomicUsize,
        stops: Arc<AtomicUsize>,
    }

    struct BlockingAdapter {
        probe_started: Notify,
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
        ) -> Result<StartedTunnel, ClientError> {
            std::future::pending().await
        }
    }

    impl RecoveryAdapter {
        fn new() -> Self {
            Self {
                blocked: StdMutex::new(BTreeSet::new()),
                tunnels: StdMutex::new(BTreeMap::new()),
                connection_attempts: AtomicUsize::new(0),
                stops: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn set_blocked<const N: usize>(&self, transport_ids: [&str; N]) {
            *self.blocked.lock().expect("blocked transport lock") =
                transport_ids.into_iter().map(str::to_owned).collect();
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
        ) -> Result<StartedTunnel, ClientError> {
            self.connection_attempts.fetch_add(1, Ordering::SeqCst);
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
                }),
                handshake_latency: Duration::from_millis(25),
            })
        }
    }

    struct RecoveryTunnel {
        transport_id: String,
        status: watch::Sender<TunnelStatus>,
        stops: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ActiveTunnel for RecoveryTunnel {
        fn transport_id(&self) -> &str {
            &self.transport_id
        }

        fn status(&self) -> watch::Receiver<TunnelStatus> {
            self.status.subscribe()
        }

        async fn stop(&self) -> Result<(), ClientError> {
            self.stops.fetch_add(1, Ordering::SeqCst);
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
