mod diagnostics;

use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, BypassList, ClientIssue, ClientSnapshot, ConnectOptions,
    ControlEndpoint, HttpControlPlane, InviteEnrollment, IpPrefix, KetClient, MaintenanceTask,
    NetworkTransportMemory, PrefixError, SelectionPolicy,
};
use ket_core::TransportProtocol;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State,
    ipc::{InvokeBody, Request},
};
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

/// What the UI needs to enroll from a verified invite. Holds the bearer
/// access code, so it is only returned to the local webview.
#[derive(Clone, Serialize)]
struct InviteDetails {
    server_url: String,
    access_code: String,
    access_expires_at_epoch_seconds: Option<u64>,
    node_key: String,
    node_fingerprint: String,
    /// The invite requires encrypted control requests.
    sealed_control: bool,
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

/// The operating system and network kind for a diagnostic report.
#[tauri::command]
fn diagnostic_context() -> diagnostics::DiagnosticContext {
    diagnostics::context()
}

/// Verifies invite text locally (signature, structure, expiry); no network.
#[tauri::command]
fn inspect_invite(invite: String) -> Result<InviteDetails, ClientIssue> {
    invite_details(&invite, unix_now())
}

fn invite_details(invite: &str, now_epoch_seconds: u64) -> Result<InviteDetails, ClientIssue> {
    let enrollment = InviteEnrollment::from_text(invite, now_epoch_seconds).map_err(issue)?;
    let node_key = enrollment
        .endpoint
        .node_key_text()
        .ok_or_else(|| issue(ket_client_core::ClientError::NodeIdentityMismatch))?;
    Ok(InviteDetails {
        server_url: enrollment
            .endpoint
            .as_str()
            .trim_end_matches('/')
            .to_owned(),
        access_code: enrollment.access_code.expose_secret().to_owned(),
        access_expires_at_epoch_seconds: enrollment.access_expires_at_epoch_seconds,
        node_key,
        node_fingerprint: enrollment.node_fingerprint,
        sealed_control: enrollment.endpoint.sealing_required(),
    })
}

/// Reads an invite QR code from a user-selected or pasted image (raw bytes,
/// never a path or URL) and verifies it like pasted invite text.
#[tauri::command]
fn decode_invite_qr(request: Request<'_>) -> Result<InviteDetails, ClientIssue> {
    match request.body() {
        InvokeBody::Raw(bytes) => invite_from_qr_image(bytes, unix_now()),
        InvokeBody::Json(_) => Err(unsupported_qr_image()),
    }
}

fn invite_from_qr_image(
    bytes: &[u8],
    now_epoch_seconds: u64,
) -> Result<InviteDetails, ClientIssue> {
    let text = ket_qr::decode_qr_image(bytes)
        .map_err(|_| unsupported_qr_image())?
        .ok_or_else(|| ClientIssue {
            code: "qr_not_found".to_owned(),
            message: "no QR code was found in that image".to_owned(),
            retryable: false,
        })?;
    invite_details(&text, now_epoch_seconds)
}

fn unsupported_qr_image() -> ClientIssue {
    ClientIssue {
        code: "unsupported_image".to_owned(),
        message: "choose a PNG or JPEG image of a Ket invite QR code (up to 8 MiB)".to_owned(),
        retryable: false,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[tauri::command]
async fn enroll(
    app: AppHandle,
    controller: State<'_, DesktopController>,
    server_url: String,
    access_code: String,
    device_name: String,
    node_key: Option<String>,
    sealed_control: Option<bool>,
) -> Result<ClientSnapshot, ClientIssue> {
    let _lifecycle = controller.lifecycle.lock().await;
    let endpoint = ControlEndpoint::parse_pinned(server_url.trim(), node_key.as_deref())
        .map_err(issue)?
        .with_required_sealing(sealed_control.unwrap_or(false));
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
    if let Ok(directory) = app.path().app_data_dir() {
        client
            .set_transport_memory(Arc::new(NetworkTransportMemory::new(
                directory.join("transport-history.json"),
            )))
            .await;
    }
    let snapshot = match client.enroll(access_code).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            // A refused app version still shows where to get the update.
            if error.required_update().is_some() {
                let _ = app.emit(SNAPSHOT_EVENT, client.snapshot());
            }
            return Err(issue(error));
        }
    };

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
    split_tls_hello: Option<bool>,
    automatic_excluded: Option<Vec<String>>,
    bypass: Option<Vec<String>>,
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
    let automatic_excluded = automatic_excluded
        .unwrap_or_default()
        .iter()
        .map(|protocol| parse_protocol(protocol))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let bypass = bypass_list(&bypass.unwrap_or_default())?;
    client
        .set_connect_options(ConnectOptions {
            split_tls_hello: split_tls_hello.unwrap_or(false),
            bypass,
        })
        .await;
    client.set_automatic_exclusions(automatic_excluded).await;
    client
        .connect_with_protocol(preferred_protocol)
        .await
        .map_err(issue)
}

/// Checks one entry a person typed for the bypass list and returns the form
/// it is kept in, such as `203.0.113.0/24` for `203.0.113.9/24`.
#[tauri::command]
fn check_bypass_entry(entry: String) -> Result<String, ClientIssue> {
    IpPrefix::parse_bypass(&entry)
        .map(|prefix| prefix.to_string())
        .map_err(bypass_issue)
}

fn bypass_list(entries: &[String]) -> Result<BypassList, ClientIssue> {
    let prefixes = entries
        .iter()
        .map(|entry| IpPrefix::parse_bypass(entry))
        .collect::<Result<Vec<_>, _>>()
        .map_err(bypass_issue)?;
    BypassList::new(prefixes).map_err(bypass_issue)
}

fn bypass_issue(error: PrefixError) -> ClientIssue {
    ClientIssue {
        code: format!("bypass_{}", error.code()),
        message: error.to_string(),
        retryable: false,
    }
}

fn parse_protocol(value: &str) -> Result<TransportProtocol, ClientIssue> {
    match value {
        "hysteria2" => Ok(TransportProtocol::Hysteria2),
        "open_vpn_stunnel" => Ok(TransportProtocol::OpenVpnStunnel),
        "shadowsocks2022" => Ok(TransportProtocol::Shadowsocks2022),
        "stealth" => Ok(TransportProtocol::Stealth),
        "vless_xtls_reality" => Ok(TransportProtocol::VlessXtlsReality),
        "wire_guard" => Ok(TransportProtocol::WireGuard),
        "xor_scrambled" => Ok(TransportProtocol::XorScrambled),
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
            diagnostic_context,
            check_bypass_entry,
            inspect_invite,
            decode_invite_qr,
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
    fn bypass_entries_are_normalized_and_refusals_named() {
        assert_eq!(
            check_bypass_entry(" 203.0.113.9/24 ".to_owned()).unwrap(),
            "203.0.113.0/24"
        );
        assert_eq!(
            check_bypass_entry("0.0.0.0/0".to_owned()).unwrap_err().code,
            "bypass_too_wide"
        );
        assert_eq!(
            bypass_list(&["10.0.0.0/8".to_owned(), "example.com".to_owned()])
                .unwrap_err()
                .code,
            "bypass_invalid"
        );
        assert_eq!(
            bypass_list(&["10.0.0.0/8".to_owned(), "10.0.0.0/8".to_owned()])
                .unwrap()
                .entries()
                .len(),
            1
        );
    }

    #[test]
    fn desktop_protocol_parser_covers_every_implemented_wire_name() {
        for (wire_name, protocol) in [
            ("hysteria2", TransportProtocol::Hysteria2),
            ("open_vpn_stunnel", TransportProtocol::OpenVpnStunnel),
            ("shadowsocks2022", TransportProtocol::Shadowsocks2022),
            ("stealth", TransportProtocol::Stealth),
            ("vless_xtls_reality", TransportProtocol::VlessXtlsReality),
            ("wire_guard", TransportProtocol::WireGuard),
            ("xor_scrambled", TransportProtocol::XorScrambled),
        ] {
            assert_eq!(parse_protocol(wire_name).unwrap(), protocol);
        }

        for invalid in ["", "auto", "ikev2", "STEALTH", " stealth"] {
            let issue = parse_protocol(invalid).unwrap_err();
            assert_eq!(issue.code, "invalid_protocol");
            assert!(!issue.retryable);
            if !invalid.is_empty() {
                assert!(!issue.message.contains(invalid));
            }
        }
    }

    #[test]
    fn invites_are_verified_locally_into_enrollment_details() {
        use ket_invite::{
            ControlLocator, KetInvite, NodeSigningKey, SignedInviteCapsule, encode_invite_text,
        };
        let key = NodeSigningKey::from_seed(&[0x31; 32]);
        let capsule = KetInvite::capsule(
            key.public_key(),
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            "Abcdefghijklmnopqrstuvwxyz012345",
            Some(2_000_000_000),
        )
        .unwrap();
        let text = encode_invite_text(&SignedInviteCapsule::sign(capsule, &key).unwrap()).unwrap();

        let details = invite_details(&text, 1_900_000_000).unwrap();
        assert_eq!(details.server_url, "https://ket.example.com");
        assert_eq!(details.access_code, "Abcdefghijklmnopqrstuvwxyz012345");
        assert_eq!(details.access_expires_at_epoch_seconds, Some(2_000_000_000));
        assert_eq!(details.node_key, key.public_key().to_base64url());
        assert_eq!(
            details.node_fingerprint,
            ket_invite::node_fingerprint(&key.public_key())
        );
        assert!(details.sealed_control, "new invites require sealed control");
        let pinned =
            ControlEndpoint::parse_pinned(&details.server_url, Some(&details.node_key)).unwrap();
        assert_eq!(pinned.node_key(), Some(&key.public_key()));

        assert_eq!(
            invite_details(&text, 2_000_000_000).err().unwrap().code,
            "invite_expired"
        );
        let tampered = format!("{}A", text.as_str());
        let rejected = invite_details(&tampered, 1).err().unwrap();
        assert!(
            !rejected
                .message
                .contains("Abcdefghijklmnopqrstuvwxyz012345")
        );
    }

    #[test]
    fn invite_qr_images_are_decoded_and_verified_natively() {
        use image::{ColorType, ImageEncoder};
        use ket_invite::{
            ControlLocator, KetInvite, NodeSigningKey, SignedInviteCapsule, encode_invite_text,
        };
        use qrcode::{Color, EcLevel, QrCode};

        let key = NodeSigningKey::from_seed(&[0x32; 32]);
        let capsule = KetInvite::capsule(
            key.public_key(),
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            "Abcdefghijklmnopqrstuvwxyz012345",
            Some(2_000_000_000),
        )
        .unwrap();
        let text = encode_invite_text(&SignedInviteCapsule::sign(capsule, &key).unwrap()).unwrap();
        let png = |content: &str| {
            let code = QrCode::with_error_correction_level(content.as_bytes(), EcLevel::M).unwrap();
            let (modules, scale, margin) = (code.width(), 5, 4);
            let colors = code.to_colors();
            let side = (modules + 2 * margin) * scale;
            let mut pixels = vec![255_u8; side * side];
            for (index, pixel) in pixels.iter_mut().enumerate() {
                let (x, y) = (index % side / scale, index / side / scale);
                let inside = (margin..modules + margin).contains(&x)
                    && (margin..modules + margin).contains(&y);
                if inside && colors[(y - margin) * modules + x - margin] == Color::Dark {
                    *pixel = 0;
                }
            }
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(&pixels, side as u32, side as u32, ColorType::L8.into())
                .unwrap();
            bytes
        };

        let details = invite_from_qr_image(&png(&text), 1_900_000_000).unwrap();
        assert_eq!(details.server_url, "https://ket.example.com");
        assert_eq!(details.node_key, key.public_key().to_base64url());

        let foreign = invite_from_qr_image(&png("https://example.com/not-an-invite"), 1);
        assert_eq!(foreign.err().unwrap().code, "invalid_invite");
        let blank = image_with_no_code();
        assert_eq!(
            invite_from_qr_image(&blank, 1).err().unwrap().code,
            "qr_not_found"
        );
        assert_eq!(
            invite_from_qr_image(b"<svg/>", 1).err().unwrap().code,
            "unsupported_image"
        );

        fn image_with_no_code() -> Vec<u8> {
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(&[255; 64 * 64], 64, 64, ColorType::L8.into())
                .unwrap();
            bytes
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
