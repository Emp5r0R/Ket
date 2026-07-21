use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Output,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, Uri, header};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use ket_core::SessionTraffic;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{net::UdpSocket, process::Command};
use zeroize::Zeroizing;

use crate::{
    config::{SHADOWSOCKS_2022_METHOD, ServerConfig, ShadowsocksConfig, XrayConfig},
    service::{SessionAllocation, unix_time},
    shadowsocks, xray,
};

const MAX_STATS_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_MANAGER_COMMAND_BYTES: usize = 4 * 1024;
const MAX_MANAGER_RESPONSE_BYTES: usize = 64 * 1024;
const SHADOWSOCKS_MANAGER_TIMEOUT: Duration = Duration::from_secs(3);
const XRAY_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub(crate) enum DataPlaneError {
    #[error("data-plane URL is invalid")]
    InvalidUrl,
    #[error("data-plane request could not be built: {0}")]
    Build(#[from] hyper::http::Error),
    #[error("data-plane request failed: {0}")]
    Request(#[from] hyper_util::client::legacy::Error),
    #[error("data-plane operation timed out")]
    Timeout,
    #[error("data-plane response body failed: {0}")]
    Body(#[from] hyper::Error),
    #[error("data-plane returned HTTP {0}")]
    Status(StatusCode),
    #[error("data-plane returned too much data")]
    ResponseTooLarge,
    #[error("data-plane response is invalid: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("data-plane I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("data-plane command failed: {0}")]
    Command(String),
    #[error("data-plane controls failed: {0}")]
    Composite(String),
}

#[async_trait]
pub(crate) trait DataPlaneControl: Send + Sync {
    async fn healthy(&self) -> bool;
    async fn provision(&self, session: &SessionAllocation) -> Result<(), DataPlaneError>;
    async fn reconcile(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError>;
    async fn traffic(&self, session: &SessionAllocation) -> Result<SessionTraffic, DataPlaneError>;
    async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError>;
}

pub(crate) fn from_config(
    config: &ServerConfig,
) -> Result<Arc<dyn DataPlaneControl>, DataPlaneError> {
    let mut controls: Vec<Arc<dyn DataPlaneControl>> = Vec::new();
    if let Some(hysteria) = &config.hysteria {
        controls.push(Arc::new(HysteriaControl::new(
            &hysteria.stats_url,
            &hysteria.stats_secret,
        )?));
    }
    if let Some(xray) = &config.xray {
        controls.push(Arc::new(XrayControl::new(xray.clone())));
    }
    if let Some(shadowsocks) = &config.shadowsocks {
        controls.push(Arc::new(ShadowsocksControl::new(shadowsocks.clone())));
    }
    match controls.len() {
        0 => Ok(Arc::new(NoDataPlane)),
        1 => Ok(controls.remove(0)),
        _ => Ok(Arc::new(CompositeDataPlane { controls })),
    }
}

struct NoDataPlane;

#[async_trait]
impl DataPlaneControl for NoDataPlane {
    async fn healthy(&self) -> bool {
        true
    }

    async fn provision(&self, _session: &SessionAllocation) -> Result<(), DataPlaneError> {
        Ok(())
    }

    async fn reconcile(&self, _sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        Ok(())
    }

    async fn traffic(
        &self,
        _session: &SessionAllocation,
    ) -> Result<SessionTraffic, DataPlaneError> {
        Ok(unavailable_traffic())
    }

    async fn kick(&self, _sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        Ok(())
    }
}

struct CompositeDataPlane {
    controls: Vec<Arc<dyn DataPlaneControl>>,
}

#[async_trait]
impl DataPlaneControl for CompositeDataPlane {
    async fn healthy(&self) -> bool {
        for control in &self.controls {
            if !control.healthy().await {
                return false;
            }
        }
        true
    }

    async fn provision(&self, session: &SessionAllocation) -> Result<(), DataPlaneError> {
        let mut completed: Vec<Arc<dyn DataPlaneControl>> = Vec::new();
        for control in &self.controls {
            if let Err(error) = control.provision(session).await {
                for provisioned in completed.into_iter().rev() {
                    let _ = provisioned.kick(std::slice::from_ref(session)).await;
                }
                return Err(error);
            }
            completed.push(Arc::clone(control));
        }
        Ok(())
    }

    async fn reconcile(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        run_all(&self.controls, |control| control.reconcile(sessions)).await
    }

    async fn traffic(&self, session: &SessionAllocation) -> Result<SessionTraffic, DataPlaneError> {
        let mut aggregate = unavailable_traffic();
        let mut failures = Vec::new();
        for control in &self.controls {
            match control.traffic(session).await {
                Ok(traffic) => {
                    aggregate.available |= traffic.available;
                    aggregate.bytes_sent = aggregate.bytes_sent.saturating_add(traffic.bytes_sent);
                    aggregate.bytes_received = aggregate
                        .bytes_received
                        .saturating_add(traffic.bytes_received);
                    aggregate.online_connections = aggregate
                        .online_connections
                        .saturating_add(traffic.online_connections);
                    aggregate.observed_at_epoch_seconds = aggregate
                        .observed_at_epoch_seconds
                        .max(traffic.observed_at_epoch_seconds);
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        if aggregate.available || failures.is_empty() {
            Ok(aggregate)
        } else {
            Err(DataPlaneError::Composite(failures.join("; ")))
        }
    }

    async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        run_all(&self.controls, |control| control.kick(sessions)).await
    }
}

async fn run_all<'a, F, Fut>(
    controls: &'a [Arc<dyn DataPlaneControl>],
    operation: F,
) -> Result<(), DataPlaneError>
where
    F: Fn(&'a Arc<dyn DataPlaneControl>) -> Fut,
    Fut: std::future::Future<Output = Result<(), DataPlaneError>>,
{
    let mut failures = Vec::new();
    for control in controls {
        if let Err(error) = operation(control).await {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(DataPlaneError::Composite(failures.join("; ")))
    }
}

fn unavailable_traffic() -> SessionTraffic {
    SessionTraffic {
        available: false,
        bytes_sent: 0,
        bytes_received: 0,
        online_connections: 0,
        observed_at_epoch_seconds: unix_time(),
    }
}

type HttpClient = Client<HttpConnector, Full<Bytes>>;

struct HysteriaControl {
    client: HttpClient,
    base_url: String,
    secret: String,
}

impl HysteriaControl {
    fn new(base_url: &str, secret: &str) -> Result<Self, DataPlaneError> {
        let base_url = base_url.trim_end_matches('/').to_owned();
        let uri: Uri = base_url.parse().map_err(|_| DataPlaneError::InvalidUrl)?;
        if uri.scheme_str() != Some("http") || uri.authority().is_none() {
            return Err(DataPlaneError::InvalidUrl);
        }
        let mut connector = HttpConnector::new();
        connector.enforce_http(true);
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            base_url,
            secret: secret.to_owned(),
        })
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, DataPlaneError>
    where
        T: serde::de::DeserializeOwned,
    {
        let request = Request::builder()
            .method(Method::GET)
            .uri(self.uri(path)?)
            .header(header::AUTHORIZATION, &self.secret)
            .body(Full::new(Bytes::new()))?;
        self.send_json(request).await
    }

    async fn send_json<T>(&self, request: Request<Full<Bytes>>) -> Result<T, DataPlaneError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = tokio::time::timeout(Duration::from_secs(2), self.client.request(request))
            .await
            .map_err(|_| DataPlaneError::Timeout)??;
        if !response.status().is_success() {
            return Err(DataPlaneError::Status(response.status()));
        }
        let bytes = response.into_body().collect().await?.to_bytes();
        if bytes.len() > MAX_STATS_RESPONSE_BYTES {
            return Err(DataPlaneError::ResponseTooLarge);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn uri(&self, path: &str) -> Result<Uri, DataPlaneError> {
        format!("{}{path}", self.base_url)
            .parse()
            .map_err(|_| DataPlaneError::InvalidUrl)
    }
}

#[derive(Deserialize)]
struct HysteriaTraffic {
    tx: u64,
    rx: u64,
}

#[async_trait]
impl DataPlaneControl for HysteriaControl {
    async fn healthy(&self) -> bool {
        self.get_json::<BTreeMap<String, u32>>("/online")
            .await
            .is_ok()
    }

    async fn provision(&self, _session: &SessionAllocation) -> Result<(), DataPlaneError> {
        Ok(())
    }

    async fn reconcile(&self, _sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        Ok(())
    }

    async fn traffic(&self, session: &SessionAllocation) -> Result<SessionTraffic, DataPlaneError> {
        let (traffic, online) = tokio::join!(
            self.get_json::<BTreeMap<String, HysteriaTraffic>>("/traffic"),
            self.get_json::<BTreeMap<String, u32>>("/online")
        );
        let traffic = traffic?;
        let online = online?;
        let counters = traffic.get(&session.id);
        Ok(SessionTraffic {
            available: true,
            bytes_sent: counters.map_or(0, |value| value.tx),
            bytes_received: counters.map_or(0, |value| value.rx),
            online_connections: online.get(&session.id).copied().unwrap_or(0),
            observed_at_epoch_seconds: unix_time(),
        })
    }

    async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        if sessions.is_empty() {
            return Ok(());
        }
        let session_ids: Vec<_> = sessions.iter().map(|session| &session.id).collect();
        let body = serde_json::to_vec(&session_ids)?;
        let request = Request::builder()
            .method(Method::POST)
            .uri(self.uri("/kick")?)
            .header(header::AUTHORIZATION, &self.secret)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))?;
        let response = tokio::time::timeout(Duration::from_secs(2), self.client.request(request))
            .await
            .map_err(|_| DataPlaneError::Timeout)??;
        if !response.status().is_success() {
            return Err(DataPlaneError::Status(response.status()));
        }
        Ok(())
    }
}

struct ShadowsocksControl {
    config: ShadowsocksConfig,
}

#[derive(Serialize)]
struct ShadowsocksAddRequest<'a> {
    server_port: u16,
    password: &'a str,
    method: &'static str,
    mode: &'static str,
}

impl ShadowsocksControl {
    fn new(config: ShadowsocksConfig) -> Self {
        Self { config }
    }

    async fn request(&self, command: &[u8]) -> Result<String, DataPlaneError> {
        if command.len() > MAX_MANAGER_COMMAND_BYTES {
            return Err(DataPlaneError::ResponseTooLarge);
        }
        tokio::time::timeout(SHADOWSOCKS_MANAGER_TIMEOUT, async {
            let target = tokio::net::lookup_host(&self.config.manager_address)
                .await?
                .next()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        "Shadowsocks manager address resolved to no endpoints",
                    )
                })?;
            let bind_address = if target.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            };
            let socket = UdpSocket::bind(bind_address).await?;
            socket.connect(target).await?;
            let sent = socket.send(command).await?;
            if sent != command.len() {
                return Err(DataPlaneError::Command(
                    "Shadowsocks manager command was truncated".to_owned(),
                ));
            }
            let mut response = vec![0_u8; MAX_MANAGER_RESPONSE_BYTES];
            let received = socket.recv(&mut response).await?;
            if received == response.len() {
                return Err(DataPlaneError::ResponseTooLarge);
            }
            response.truncate(received);
            String::from_utf8(response).map_err(|_| {
                DataPlaneError::Command("Shadowsocks manager returned non-UTF-8 data".to_owned())
            })
        })
        .await
        .map_err(|_| DataPlaneError::Timeout)?
    }

    async fn active_ports(&self) -> Result<BTreeSet<u16>, DataPlaneError> {
        parse_shadowsocks_ping(&self.request(b"ping\n").await?)
    }

    async fn add_server(&self, session: &SessionAllocation) -> Result<u16, DataPlaneError> {
        let port = self.session_port(session)?;
        let password = shadowsocks::session_key(&self.config, &session.id);
        let encoded = Zeroizing::new(serde_json::to_vec(&ShadowsocksAddRequest {
            server_port: port,
            password: password.expose_secret(),
            method: SHADOWSOCKS_2022_METHOD,
            mode: "tcp_and_udp",
        })?);
        let mut command = Zeroizing::new(Vec::with_capacity(encoded.len() + 7));
        command.extend_from_slice(b"add: ");
        command.extend_from_slice(&encoded);
        command.push(b'\n');
        expect_shadowsocks_ok(&self.request(&command).await?)?;
        Ok(port)
    }

    async fn remove_server(&self, port: u16) -> Result<(), DataPlaneError> {
        let command = format!("remove: {{\"server_port\":{port}}}\n");
        expect_shadowsocks_ok(&self.request(command.as_bytes()).await?)
    }

    fn session_port(&self, session: &SessionAllocation) -> Result<u16, DataPlaneError> {
        self.config
            .session_port(session.resource_slot)
            .ok_or_else(|| {
                DataPlaneError::Command(
                    "session resource slot exceeds the Shadowsocks port pool".to_owned(),
                )
            })
    }

    fn managed_port(&self, port: u16) -> bool {
        (self.config.port_start..=self.config.port_end).contains(&port)
    }
}

#[async_trait]
impl DataPlaneControl for ShadowsocksControl {
    async fn healthy(&self) -> bool {
        self.active_ports().await.is_ok()
    }

    async fn provision(&self, session: &SessionAllocation) -> Result<(), DataPlaneError> {
        let port = self.session_port(session)?;
        self.remove_server(port).await?;
        self.add_server(session).await?;
        if !self.active_ports().await?.contains(&port) {
            return Err(DataPlaneError::Command(
                "Shadowsocks manager did not retain the provisioned lease".to_owned(),
            ));
        }
        Ok(())
    }

    async fn reconcile(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        for port in self
            .active_ports()
            .await?
            .into_iter()
            .filter(|port| self.managed_port(*port))
        {
            self.remove_server(port).await?;
        }
        let mut expected = BTreeSet::new();
        for session in sessions {
            expected.insert(self.add_server(session).await?);
        }
        let actual = self
            .active_ports()
            .await?
            .into_iter()
            .filter(|port| self.managed_port(*port))
            .collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(DataPlaneError::Command(
                "Shadowsocks manager reconciliation postcondition failed".to_owned(),
            ));
        }
        Ok(())
    }

    async fn traffic(
        &self,
        _session: &SessionAllocation,
    ) -> Result<SessionTraffic, DataPlaneError> {
        // ssmanager exposes only tx+rx, so Ket cannot truthfully fill directional fields.
        Ok(unavailable_traffic())
    }

    async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        let mut removed = BTreeSet::new();
        for session in sessions {
            let port = self.session_port(session)?;
            self.remove_server(port).await?;
            removed.insert(port);
        }
        if self
            .active_ports()
            .await?
            .iter()
            .any(|port| removed.contains(port))
        {
            return Err(DataPlaneError::Command(
                "Shadowsocks manager retained a revoked lease".to_owned(),
            ));
        }
        Ok(())
    }
}

fn expect_shadowsocks_ok(response: &str) -> Result<(), DataPlaneError> {
    if response.trim() == "ok" {
        Ok(())
    } else {
        Err(DataPlaneError::Command(
            "Shadowsocks manager rejected the operation".to_owned(),
        ))
    }
}

fn parse_shadowsocks_ping(response: &str) -> Result<BTreeSet<u16>, DataPlaneError> {
    let response = response.trim();
    let payload = response.strip_prefix("stat:").ok_or_else(|| {
        DataPlaneError::Command("Shadowsocks manager returned an invalid ping response".to_owned())
    })?;
    let counters: BTreeMap<u16, u64> = serde_json::from_str(payload.trim())?;
    Ok(counters.into_keys().collect())
}

struct XrayControl {
    config: XrayConfig,
}

impl XrayControl {
    fn new(config: XrayConfig) -> Self {
        Self { config }
    }

    async fn run(&self, arguments: &[String]) -> Result<String, DataPlaneError> {
        let mut command = Command::new(&self.config.binary_path);
        command
            .args(arguments)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let output = tokio::time::timeout(XRAY_COMMAND_TIMEOUT, command.output())
            .await
            .map_err(|_| DataPlaneError::Timeout)??;
        decode_command_output(output)
    }

    fn api_arguments(&self, operation: &str) -> Vec<String> {
        vec![
            "api".to_owned(),
            operation.to_owned(),
            format!("--server={}", self.config.api_server),
            "--timeout=3".to_owned(),
        ]
    }

    async fn add_user(&self, session_id: &str) -> Result<(), DataPlaneError> {
        validate_session_id(session_id)?;
        let path = user_config_path(session_id);
        let email = xray::session_email(session_id);
        let mut added_tags = Vec::new();
        for (tag, document) in xray::inbound_tags(&self.config)
            .into_iter()
            .zip(xray::user_documents(&self.config, session_id))
        {
            if let Err(error) = write_user_config(&path, &document).await {
                let _ = self.remove_emails_from_tags(&added_tags, &[email]).await;
                return Err(error);
            }
            let mut arguments = self.api_arguments("adu");
            arguments.push(path.to_string_lossy().into_owned());
            let result = self.run(&arguments).await;
            let cleanup = tokio::fs::remove_file(&path).await;
            if let Err(error) = result {
                let _ = self.remove_emails_from_tags(&added_tags, &[email]).await;
                return Err(error);
            }
            added_tags.push(tag);
            if let Err(error) = cleanup {
                let _ = self.remove_emails_from_tags(&added_tags, &[email]).await;
                return Err(DataPlaneError::Io(error));
            }
            let inbound_emails = match self.inbound_emails(tag).await {
                Ok(inbound_emails) => inbound_emails,
                Err(error) => {
                    let _ = self.remove_emails_from_tags(&added_tags, &[email]).await;
                    return Err(error);
                }
            };
            if !inbound_emails.contains(&email) {
                let _ = self.remove_emails_from_tags(&added_tags, &[email]).await;
                return Err(DataPlaneError::Command(
                    "Xray did not retain the provisioned user".to_owned(),
                ));
            }
        }
        Ok(())
    }

    async fn remove_emails(&self, emails: &[String]) -> Result<(), DataPlaneError> {
        self.remove_emails_from_tags(&xray::inbound_tags(&self.config), emails)
            .await
    }

    async fn remove_emails_from_tags(
        &self,
        tags: &[&str],
        emails: &[String],
    ) -> Result<(), DataPlaneError> {
        for tag in tags {
            for chunk in emails.chunks(100) {
                if chunk.is_empty() {
                    continue;
                }
                let mut arguments = self.api_arguments("rmu");
                arguments.push(format!("-tag={tag}"));
                arguments.extend(chunk.iter().cloned());
                self.run(&arguments).await?;
                let inbound_emails = self.inbound_emails(tag).await?;
                if chunk.iter().any(|email| inbound_emails.contains(email)) {
                    return Err(DataPlaneError::Command(
                        "Xray retained a removed user".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    async fn managed_emails(&self) -> Result<Vec<String>, DataPlaneError> {
        let mut emails = BTreeSet::new();
        for tag in xray::inbound_tags(&self.config) {
            emails.extend(self.inbound_emails(tag).await?);
        }
        Ok(emails
            .into_iter()
            .filter(|email| email.starts_with("session-") && email.ends_with("@ket.invalid"))
            .collect())
    }

    async fn inbound_emails(&self, tag: &str) -> Result<BTreeSet<String>, DataPlaneError> {
        let mut arguments = self.api_arguments("inbounduser");
        arguments.push(format!("-tag={tag}"));
        let output = self.run(&arguments).await?;
        let value: Value = serde_json::from_str(&output)?;
        let mut emails = BTreeSet::new();
        collect_emails(&value, &mut emails);
        Ok(emails)
    }
}

#[async_trait]
impl DataPlaneControl for XrayControl {
    async fn healthy(&self) -> bool {
        for tag in xray::inbound_tags(&self.config) {
            let mut arguments = self.api_arguments("inboundusercount");
            arguments.push(format!("-tag={tag}"));
            if self.run(&arguments).await.is_err() {
                return false;
            }
        }
        true
    }

    async fn provision(&self, session: &SessionAllocation) -> Result<(), DataPlaneError> {
        self.add_user(&session.id).await
    }

    async fn reconcile(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        let existing = self.managed_emails().await?;
        self.remove_emails(&existing).await?;
        for session in sessions {
            self.add_user(&session.id).await?;
        }
        Ok(())
    }

    async fn traffic(&self, session: &SessionAllocation) -> Result<SessionTraffic, DataPlaneError> {
        validate_session_id(&session.id)?;
        let email = xray::session_email(&session.id);
        let pattern = format!("user>>>{email}>>>traffic>>>");
        let mut stats_arguments = self.api_arguments("statsquery");
        stats_arguments.push(format!("-pattern={pattern}"));
        let mut online_arguments = self.api_arguments("statsonline");
        online_arguments.push(format!("-email={email}"));
        let (stats, online) = tokio::join!(self.run(&stats_arguments), self.run(&online_arguments));
        let (bytes_sent, bytes_received) = parse_xray_stats(&stats?)?;
        let online_connections = parse_xray_online_result(online)?;
        Ok(SessionTraffic {
            available: true,
            bytes_sent,
            bytes_received,
            online_connections,
            observed_at_epoch_seconds: unix_time(),
        })
    }

    async fn kick(&self, sessions: &[SessionAllocation]) -> Result<(), DataPlaneError> {
        let emails = sessions
            .iter()
            .map(|session| {
                validate_session_id(&session.id)?;
                Ok(xray::session_email(&session.id))
            })
            .collect::<Result<Vec<_>, DataPlaneError>>()?;
        self.remove_emails(&emails).await
    }
}

fn decode_command_output(output: Output) -> Result<String, DataPlaneError> {
    if output.stdout.len().saturating_add(output.stderr.len()) > MAX_COMMAND_OUTPUT_BYTES {
        return Err(DataPlaneError::ResponseTooLarge);
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.lines().last().unwrap_or("unknown Xray API error");
        return Err(DataPlaneError::Command(message.chars().take(512).collect()));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| DataPlaneError::Command("Xray returned non-UTF-8 output".to_owned()))
}

fn validate_session_id(session_id: &str) -> Result<(), DataPlaneError> {
    if session_id.len() != 12 || !session_id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(DataPlaneError::Command(
            "session ID is invalid for Xray provisioning".to_owned(),
        ));
    }
    Ok(())
}

fn user_config_path(session_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ket-xray-user-{session_id}.json"))
}

async fn write_user_config(path: &Path, document: &Value) -> Result<(), DataPlaneError> {
    let path = path.to_owned();
    let encoded = serde_json::to_vec(document)?;
    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&encoded)?;
        file.sync_all()
    })
    .await
    .map_err(|error| DataPlaneError::Command(format!("Xray writer failed: {error}")))??;
    Ok(())
}

fn collect_emails(value: &Value, emails: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(email) = object.get("email").and_then(Value::as_str) {
                emails.insert(email.to_owned());
            }
            for nested in object.values() {
                collect_emails(nested, emails);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_emails(nested, emails);
            }
        }
        _ => {}
    }
}

fn parse_xray_stats(output: &str) -> Result<(u64, u64), DataPlaneError> {
    let value: Value = serde_json::from_str(output)?;
    let mut sent = 0_u64;
    let mut received = 0_u64;
    collect_stats(&value, &mut sent, &mut received);
    Ok((sent, received))
}

fn collect_stats(value: &Value, sent: &mut u64, received: &mut u64) {
    match value {
        Value::Object(object) => {
            if let (Some(name), Some(value)) = (
                object.get("name").and_then(Value::as_str),
                object.get("value").and_then(json_u64),
            ) {
                if name.ends_with(">>>uplink") {
                    *sent = sent.saturating_add(value);
                } else if name.ends_with(">>>downlink") {
                    *received = received.saturating_add(value);
                }
            }
            for nested in object.values() {
                collect_stats(nested, sent, received);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_stats(nested, sent, received);
            }
        }
        _ => {}
    }
}

fn parse_xray_online(output: &str) -> Result<u32, DataPlaneError> {
    let value: Value = serde_json::from_str(output)?;
    find_numeric_field(&value, &["value", "count", "online"])
        .unwrap_or(0)
        .try_into()
        .map_err(|_| DataPlaneError::Command("Xray online count exceeds u32".to_owned()))
}

fn parse_xray_online_result(output: Result<String, DataPlaneError>) -> Result<u32, DataPlaneError> {
    match output {
        Ok(output) => parse_xray_online(&output),
        Err(DataPlaneError::Command(message))
            if message.contains("code = NotFound") && message.contains(">>>online not found") =>
        {
            Ok(0)
        }
        Err(error) => Err(error),
    }
}

fn find_numeric_field(value: &Value, names: &[&str]) -> Option<u64> {
    match value {
        Value::Object(object) => {
            for name in names {
                if let Some(value) = object.get(*name).and_then(json_u64) {
                    return Some(value);
                }
            }
            object
                .values()
                .find_map(|nested| find_numeric_field(nested, names))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|nested| find_numeric_field(nested, names)),
        _ => None,
    }
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xray_stats_parser_accepts_proto_json_integer_strings() {
        let (sent, received) = parse_xray_stats(
            r#"{"stat":[{"name":"user>>>a>>>traffic>>>uplink","value":"42"},{"name":"user>>>a>>>traffic>>>downlink","value":19}]}"#,
        )
        .expect("stats should parse");
        assert_eq!((sent, received), (42, 19));
        assert_eq!(parse_xray_online(r#"{"value":"3"}"#).unwrap(), 3);
        assert_eq!(
            parse_xray_online_result(Err(DataPlaneError::Command(
                "rpc error: code = NotFound desc = user>>>a>>>online not found".to_owned(),
            )))
            .unwrap(),
            0
        );
        assert!(
            parse_xray_online_result(Err(DataPlaneError::Command(
                "rpc error: code = Unavailable".to_owned(),
            )))
            .is_err()
        );
    }

    #[test]
    fn managed_email_collector_ignores_unowned_users() {
        let value = serde_json::json!({
            "users": [
                {"email": "session-AbCdEf123456@ket.invalid"},
                {"email": "operator@example.com"}
            ]
        });
        let mut emails = BTreeSet::new();
        collect_emails(&value, &mut emails);
        let managed: Vec<_> = emails
            .into_iter()
            .filter(|email| email.starts_with("session-") && email.ends_with("@ket.invalid"))
            .collect();
        assert_eq!(managed, ["session-AbCdEf123456@ket.invalid"]);
    }

    #[test]
    fn session_ids_cannot_escape_the_temporary_directory() {
        assert!(validate_session_id("AbCdEf123456").is_ok());
        assert!(validate_session_id("../../etc/pass").is_err());
        assert!(validate_session_id("too-short").is_err());
    }

    #[test]
    fn shadowsocks_ping_parser_accepts_only_the_manager_contract() {
        assert_eq!(
            parse_shadowsocks_ping("stat: {\"20000\":0,\"20001\":42}\n").unwrap(),
            BTreeSet::from([20_000, 20_001])
        );
        assert!(parse_shadowsocks_ping("ok\n").is_err());
        assert!(parse_shadowsocks_ping("stat: {\"70000\":0}\n").is_err());
        assert!(parse_shadowsocks_ping("stat: []\n").is_err());
        assert!(expect_shadowsocks_ok("ok\n").is_ok());
        assert!(expect_shadowsocks_ok("err: rejected\n").is_err());
    }

    #[tokio::test]
    async fn pinned_shadowsocks_manager_reconciles_and_revokes_when_supplied() {
        let Some(binary) = std::env::var_os("KET_TEST_SHADOWSOCKS_MANAGER_BINARY") else {
            return;
        };
        let manager_port = reserve_udp_port();
        let relay_start = reserve_dual_port_pair();
        let child = std::process::Command::new(binary)
            .args([
                "-U",
                "--server-host",
                "127.0.0.1",
                "--manager-addr",
                &format!("127.0.0.1:{manager_port}"),
                "--encrypt-method",
                SHADOWSOCKS_2022_METHOD,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start pinned Shadowsocks manager");
        let _manager = TestChild(child);
        let control = ShadowsocksControl::new(ShadowsocksConfig {
            transport_id: "shadowsocks-2022-primary".to_owned(),
            manager_address: format!("127.0.0.1:{manager_port}"),
            public_host: "127.0.0.1".to_owned(),
            port_start: relay_start,
            port_end: relay_start + 1,
            credential_key: "live-test-key-with-at-least-32-characters".to_owned(),
        });
        for _ in 0..50 {
            if control.healthy().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(control.healthy().await, "manager did not become ready");

        let sessions = [
            SessionAllocation {
                id: "AbCdEf123456".to_owned(),
                resource_slot: 0,
            },
            SessionAllocation {
                id: "GhIjKl789012".to_owned(),
                resource_slot: 1,
            },
        ];
        control
            .reconcile(&sessions)
            .await
            .expect("reconcile live Shadowsocks leases");
        assert_eq!(
            control.active_ports().await.unwrap(),
            BTreeSet::from([relay_start, relay_start + 1])
        );
        for port in [relay_start, relay_start + 1] {
            assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());
            assert!(std::net::UdpSocket::bind(("127.0.0.1", port)).is_err());
        }

        control
            .kick(&sessions)
            .await
            .expect("revoke live Shadowsocks leases");
        assert!(control.active_ports().await.unwrap().is_empty());
    }

    struct TestChild(std::process::Child);

    impl Drop for TestChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn reserve_udp_port() -> u16 {
        std::net::UdpSocket::bind(("127.0.0.1", 0))
            .expect("reserve UDP manager port")
            .local_addr()
            .expect("reserved manager address")
            .port()
    }

    fn reserve_dual_port_pair() -> u16 {
        for _ in 0..100 {
            let tcp =
                std::net::TcpListener::bind(("127.0.0.1", 0)).expect("reserve candidate TCP port");
            let start = tcp.local_addr().expect("candidate TCP address").port();
            if start == u16::MAX {
                continue;
            }
            let Ok(udp) = std::net::UdpSocket::bind(("127.0.0.1", start)) else {
                continue;
            };
            let Ok(next_tcp) = std::net::TcpListener::bind(("127.0.0.1", start + 1)) else {
                continue;
            };
            let Ok(next_udp) = std::net::UdpSocket::bind(("127.0.0.1", start + 1)) else {
                continue;
            };
            drop((tcp, udp, next_tcp, next_udp));
            return start;
        }
        panic!("could not reserve consecutive TCP+UDP relay ports");
    }
}
