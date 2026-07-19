use std::{sync::Arc, time::Duration};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, ClientIssue, ClientSnapshot, ControlEndpoint,
    HttpControlPlane, KetClient, MaintenanceTask, SelectionPolicy,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{sync::Mutex, task::JoinHandle};

const SNAPSHOT_EVENT: &str = "ket://snapshot";

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
    session: Mutex<Option<DesktopSession>>,
}

#[derive(Clone, Debug, Serialize)]
struct EngineReadiness {
    binary_available: bool,
    broker_available: bool,
    mode: &'static str,
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
            session: Mutex::new(None),
        }
    }

    async fn readiness(&self) -> EngineReadiness {
        match self.adapter.readiness().await {
            Ok(readiness) => EngineReadiness {
                binary_available: readiness.engine_available,
                broker_available: true,
                mode: "privileged_broker",
            },
            Err(error) => {
                tracing::debug!(error = %error, "privileged tunnel service is unavailable");
                EngineReadiness {
                    binary_available: false,
                    broker_available: false,
                    mode: "privileged_broker",
                }
            }
        }
    }
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
    let endpoint = ControlEndpoint::parse(server_url.trim()).map_err(issue)?;
    let api = Arc::new(HttpControlPlane::new().map_err(issue)?);
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
    let previous = controller.session.lock().await.replace(DesktopSession {
        client,
        maintenance: None,
        watcher,
    });
    if let Some(previous) = previous {
        let _ = previous.client.disconnect().await;
    }
    let _ = app.emit(SNAPSHOT_EVENT, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
async fn connect(controller: State<'_, DesktopController>) -> Result<ClientSnapshot, ClientIssue> {
    let mut session = controller.session.lock().await;
    let session = session.as_mut().ok_or_else(not_enrolled)?;
    let snapshot = session.client.connect().await.map_err(issue)?;
    if session.maintenance.is_none() {
        session.maintenance = Some(session.client.spawn_maintenance(Duration::from_secs(15)));
    }
    Ok(snapshot)
}

#[tauri::command]
async fn stop_tunnel(
    controller: State<'_, DesktopController>,
) -> Result<ClientSnapshot, ClientIssue> {
    let session = controller.session.lock().await;
    let session = session.as_ref().ok_or_else(not_enrolled)?;
    session.client.stop_tunnel().await.map_err(issue)
}

#[tauri::command]
async fn refresh(controller: State<'_, DesktopController>) -> Result<ClientSnapshot, ClientIssue> {
    let session = controller.session.lock().await;
    let session = session.as_ref().ok_or_else(not_enrolled)?;
    session.client.refresh().await.map_err(issue)
}

#[tauri::command]
async fn forget(controller: State<'_, DesktopController>) -> Result<ClientSnapshot, ClientIssue> {
    let session = controller.session.lock().await.take();
    let Some(mut session) = session else {
        return Ok(ClientSnapshot::default());
    };
    if let Some(maintenance) = session.maintenance.take() {
        maintenance.shutdown().await;
    }
    session.client.disconnect().await.map_err(issue)
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
    tauri::Builder::default()
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
        .run(tauri::generate_context!())
        .expect("failed to run Ket desktop");
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
