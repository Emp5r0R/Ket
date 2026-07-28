use std::{sync::Arc, time::Duration};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, ClientIssue, ClientSnapshot, ControlEndpoint,
    HttpControlPlane, KetClient, MaintenanceTask, SelectionPolicy,
};
use ket_core::TransportProtocol;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tokio::{sync::Mutex, task::JoinHandle};

const SNAPSHOT_EVENT: &str = "ket://snapshot";
const EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

struct DesktopSession {
    client: Arc<KetClient>,
    maintenance: Option<MaintenanceTask>,
    watcher: JoinHandle<()>,
}

impl Drop for DesktopSession {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

struct DesktopController {
    adapter: BrokerTransportAdapter,
    lifecycle: Mutex<()>,
    session: Mutex<Option<DesktopSession>>,
}

#[derive(Clone, Debug, Serialize)]
struct EngineReadiness {
    binary_available: bool,
    broker_available: bool,
    mode: &'static str,
    status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct DesktopState {
    snapshot: ClientSnapshot,
    configured: bool,
    engine: EngineReadiness,
    platform: &'static str,
    version: &'static str,
}

impl DesktopController {
    fn new(config: BrokerConfig) -> Self {
        Self {
            adapter: BrokerTransportAdapter::new(config),
            lifecycle: Mutex::new(()),
            session: Mutex::new(None),
        }
    }

    async fn readiness(&self) -> EngineReadiness {
        match self.adapter.readiness().await {
            Ok(readiness) => EngineReadiness {
                binary_available: readiness.engine_available,
                broker_available: true,
                mode: "privileged_broker",
                status: "ready",
            },
            Err(error) => {
                tracing::debug!(error = %error, "privileged tunnel service is unavailable");
                EngineReadiness {
                    binary_available: false,
                    broker_available: false,
                    mode: "privileged_broker",
                    status: readiness_error_status(&error),
                }
            }
        }
    }
}

fn readiness_error_status(error: &ket_client_core::ClientError) -> &'static str {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("credentials are unavailable")
        || message.contains("permission denied")
        || message.contains("authentication failed")
    {
        "permission_required"
    } else if tunnel_service_installed() {
        "unavailable"
    } else {
        "not_installed"
    }
}

#[cfg(target_os = "linux")]
fn tunnel_service_installed() -> bool {
    std::path::Path::new("/usr/libexec/ket/ket-tunnel-service").is_file()
}

#[cfg(target_os = "windows")]
fn tunnel_service_installed() -> bool {
    std::env::var_os("PROGRAMFILES")
        .map(std::path::PathBuf::from)
        .map(|path| path.join("Ket").join("ket-tunnel-service.exe").is_file())
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn tunnel_service_installed() -> bool {
    false
}

#[tauri::command]
async fn desktop_state(
    controller: State<'_, DesktopController>,
) -> Result<DesktopState, ClientIssue> {
    let (snapshot, configured) = {
        let session = controller.session.lock().await;
        (
            session
                .as_ref()
                .map_or_else(ClientSnapshot::default, |session| session.client.snapshot()),
            session.is_some(),
        )
    };
    Ok(DesktopState {
        snapshot,
        configured,
        engine: controller.readiness().await,
        platform: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[tauri::command]
async fn enroll(
    app: AppHandle,
    controller: State<'_, DesktopController>,
    server_url: String,
    access_code: String,
    device_name: String,
) -> Result<ClientSnapshot, ClientIssue> {
    let _lifecycle = controller.lifecycle.lock().await;
    let endpoint = ControlEndpoint::parse(server_url.trim()).map_err(issue)?;
    let api = Arc::new(HttpControlPlane::new().map_err(issue)?);
    retire_current_session(&controller).await?;
    let adapter = Arc::new(controller.adapter.clone());
    let client = KetClient::new(
        endpoint,
        device_name,
        api,
        vec![adapter],
        SelectionPolicy::default(),
    )
    .map_err(issue)?;
    let snapshot = client.enroll(access_code).await.map_err(issue)?;

    let mut receiver = client.subscribe();
    let watcher_app = app.clone();
    let watcher = tokio::spawn(async move {
        while receiver.changed().await.is_ok() {
            let _ = watcher_app.emit(SNAPSHOT_EVENT, receiver.borrow().clone());
        }
    });
    let maintenance = client.spawn_maintenance(Duration::from_secs(15));
    let previous = controller.session.lock().await.replace(DesktopSession {
        client,
        maintenance: Some(maintenance),
        watcher,
    });
    debug_assert!(
        previous.is_none(),
        "the previous desktop session was retired"
    );
    let _ = app.emit(SNAPSHOT_EVENT, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
async fn connect(
    controller: State<'_, DesktopController>,
    preferred_protocol: Option<String>,
) -> Result<ClientSnapshot, ClientIssue> {
    let client = controller
        .session
        .lock()
        .await
        .as_ref()
        .map(|session| Arc::clone(&session.client))
        .ok_or_else(not_enrolled)?;
    let preferred_protocol = preferred_protocol
        .as_deref()
        .map(parse_protocol)
        .transpose()?;
    client
        .connect_with_protocol(preferred_protocol)
        .await
        .map_err(issue)
}

fn parse_protocol(value: &str) -> Result<TransportProtocol, ClientIssue> {
    match value {
        "hysteria2" => Ok(TransportProtocol::Hysteria2),
        "open_vpn_stunnel" => Ok(TransportProtocol::OpenVpnStunnel),
        "shadowsocks2022" => Ok(TransportProtocol::Shadowsocks2022),
        "stealth" => Ok(TransportProtocol::Stealth),
        "vless_xtls_reality" => Ok(TransportProtocol::VlessXtlsReality),
        "wire_guard" => Ok(TransportProtocol::WireGuard),
        _ => Err(ClientIssue {
            code: "invalid_protocol".to_owned(),
            message: "choose a protocol offered by this server".to_owned(),
            retryable: false,
        }),
    }
}

#[tauri::command]
async fn stop_tunnel(
    controller: State<'_, DesktopController>,
) -> Result<ClientSnapshot, ClientIssue> {
    let client = controller
        .session
        .lock()
        .await
        .as_ref()
        .map(|session| Arc::clone(&session.client))
        .ok_or_else(not_enrolled)?;
    client.stop_tunnel().await.map_err(issue)
}

#[tauri::command]
async fn refresh(controller: State<'_, DesktopController>) -> Result<ClientSnapshot, ClientIssue> {
    let client = controller
        .session
        .lock()
        .await
        .as_ref()
        .map(|session| Arc::clone(&session.client))
        .ok_or_else(not_enrolled)?;
    client.refresh().await.map_err(issue)
}

#[tauri::command]
async fn forget(controller: State<'_, DesktopController>) -> Result<ClientSnapshot, ClientIssue> {
    let _lifecycle = controller.lifecycle.lock().await;
    Ok(retire_current_session(&controller)
        .await?
        .unwrap_or_default())
}

async fn retire_current_session(
    controller: &DesktopController,
) -> Result<Option<ClientSnapshot>, ClientIssue> {
    let session = controller.session.lock().await.take();
    let Some(mut session) = session else {
        return Ok(None);
    };
    if let Some(maintenance) = session.maintenance.take() {
        maintenance.shutdown().await;
    }
    match session.client.disconnect().await {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(error) => {
            session.maintenance = Some(session.client.spawn_maintenance(Duration::from_secs(15)));
            let replaced = controller.session.lock().await.replace(session);
            debug_assert!(
                replaced.is_none(),
                "desktop lifecycle operations are serialized"
            );
            Err(issue(error))
        }
    }
}

async fn shutdown_controller(controller: &DesktopController) {
    let _lifecycle = controller.lifecycle.lock().await;
    let session = controller.session.lock().await.take();
    let Some(mut session) = session else {
        return;
    };
    if let Some(maintenance) = session.maintenance.take() {
        drop(maintenance);
    }
    let _ = session.client.disconnect().await;
}

fn release_session_before_exit(app: &AppHandle) {
    let app = app.clone();
    let (finished, receiver) = std::sync::mpsc::sync_channel(1);
    tauri::async_runtime::spawn(async move {
        let controller = app.state::<DesktopController>();
        let _ = tokio::time::timeout(EXIT_CLEANUP_TIMEOUT, shutdown_controller(&controller)).await;
        let _ = finished.send(());
    });
    let _ = receiver.recv_timeout(EXIT_CLEANUP_TIMEOUT + Duration::from_secs(1));
}

fn issue(error: ket_client_core::ClientError) -> ClientIssue {
    error.issue()
}

fn not_enrolled() -> ClientIssue {
    ClientIssue {
        code: "not_enrolled".to_owned(),
        message: "enter a server and access code first".to_owned(),
        retryable: false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            app.manage(DesktopController::new(BrokerConfig::from_env()?));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_state,
            enroll,
            connect,
            stop_tunnel,
            refresh,
            forget,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Ket desktop");
    app.run(|app, event| {
        if matches!(event, RunEvent::ExitRequested { .. }) {
            release_session_before_exit(app);
        }
    });
}

#[cfg(test)]
mod command_contract_tests {
    use super::*;

    #[test]
    fn desktop_protocol_parser_covers_every_implemented_wire_name() {
        for (wire_name, protocol) in [
            ("hysteria2", TransportProtocol::Hysteria2),
            ("open_vpn_stunnel", TransportProtocol::OpenVpnStunnel),
            ("shadowsocks2022", TransportProtocol::Shadowsocks2022),
            ("stealth", TransportProtocol::Stealth),
            ("vless_xtls_reality", TransportProtocol::VlessXtlsReality),
            ("wire_guard", TransportProtocol::WireGuard),
        ] {
            assert_eq!(parse_protocol(wire_name).unwrap(), protocol);
        }

        for invalid in ["", "auto", "ikev2", "xor_scrambled", "STEALTH", " stealth"] {
            let issue = parse_protocol(invalid).unwrap_err();
            assert_eq!(issue.code, "invalid_protocol");
            assert!(!issue.retryable);
            if !invalid.is_empty() {
                assert!(!issue.message.contains(invalid));
            }
        }
    }

    #[test]
    fn command_errors_keep_stable_machine_readable_codes() {
        let missing = not_enrolled();
        assert_eq!(missing.code, "not_enrolled");
        assert!(!missing.retryable);

        let invalid = issue(ket_client_core::ClientError::InvalidInput(
            "bad code".to_owned(),
        ));
        assert_eq!(invalid.code, "invalid_input");
        assert!(!invalid.retryable);
        assert!(invalid.message.contains("input is invalid"));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod security_regression_tests {
    use glib::{Variant, prelude::*};

    #[test]
    fn patched_glib_variant_str_iterator_handles_forward_and_reverse() {
        let variant = Variant::array_from_iter::<String>([
            "alpha".to_variant(),
            "beta".to_variant(),
            "gamma".to_variant(),
        ]);
        let mut values = variant.array_iter_str().expect("string array variant");

        assert_eq!(values.next(), Some("alpha"));
        assert_eq!(values.next_back(), Some("gamma"));
        assert_eq!(values.next(), Some("beta"));
        assert_eq!(values.next(), None);
    }
}
