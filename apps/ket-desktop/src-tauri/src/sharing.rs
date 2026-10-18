//! "Share with nearby devices": the commands the UI uses to list the
//! networks this computer can share on, start and stop the share, and follow
//! how many devices use it. The share itself lives in `ket-client-core`.

use std::net::Ipv4Addr;

use ket_client_core::{
    ClientIssue, ShareDetails, ShareError, ShareNetwork, ShareSession, ShareStatus, share_networks,
};
use qrcode::{EcLevel, QrCode, render::svg};
use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

#[derive(Default)]
pub(crate) struct ShareController {
    session: Mutex<Option<ShareSession>>,
}

impl ShareController {
    /// Ends sharing, if it is on; used when the user disconnects or forgets
    /// the server, and when the app exits.
    pub(crate) async fn stop(&self) {
        if let Some(session) = self.session.lock().await.take() {
            session.stop().await;
        }
    }
}

/// What the UI shows while sharing: how to connect, as text and as a QR
/// code holding the proxy address with its sign-in.
#[derive(Clone, Serialize)]
pub(crate) struct ShareView {
    details: ShareDetails,
    /// `http://ket:PASSWORD@ADDRESS:PORT`, which proxy apps can import.
    proxy_url: String,
    qr_svg: String,
}

#[tauri::command]
pub(crate) fn list_share_networks() -> Vec<ShareNetwork> {
    share_networks()
}

#[tauri::command]
pub(crate) async fn start_sharing(
    address: String,
    sharing: State<'_, ShareController>,
) -> Result<ShareView, ClientIssue> {
    let address = address
        .parse::<Ipv4Addr>()
        .map_err(|_| share_issue(&ShareError::NetworkUnavailable))?;
    let mut session = sharing.session.lock().await;
    if let Some(previous) = session.take() {
        previous.stop().await;
    }
    let started = ShareSession::start(address)
        .await
        .map_err(|error| share_issue(&error))?;
    let view = share_view(started.details());
    *session = Some(started);
    Ok(view)
}

#[tauri::command]
pub(crate) async fn stop_sharing(sharing: State<'_, ShareController>) -> Result<(), ClientIssue> {
    sharing.stop().await;
    Ok(())
}

/// `None` when sharing is off. A share that ended on its own (its network
/// went away) reports why once and is then cleared.
#[tauri::command]
pub(crate) async fn share_status(
    sharing: State<'_, ShareController>,
) -> Result<Option<ShareStatus>, ClientIssue> {
    let mut session = sharing.session.lock().await;
    let Some(status) = session.as_ref().map(ShareSession::status) else {
        return Ok(None);
    };
    if status.ended.is_some()
        && let Some(ended) = session.take()
    {
        ended.stop().await;
    }
    Ok(Some(status))
}

fn share_view(details: &ShareDetails) -> ShareView {
    let proxy_url = proxy_url(details);
    ShareView {
        details: details.clone(),
        qr_svg: qr_svg(&proxy_url),
        proxy_url,
    }
}

fn proxy_url(details: &ShareDetails) -> String {
    format!(
        "http://{}:{}@{}:{}",
        details.username, details.password, details.network.address, details.port
    )
}

fn qr_svg(text: &str) -> String {
    QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map(|code| {
            code.render::<svg::Color<'_>>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .dark_color(svg::Color("#000000"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default()
}

fn share_issue(error: &ShareError) -> ClientIssue {
    ClientIssue {
        code: format!("share_{}", error.code()),
        message: error.to_string(),
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    use ket_client_core::NetworkKind;

    use super::*;

    fn details() -> ShareDetails {
        ShareDetails {
            network: ShareNetwork {
                interface: "wlan0".to_owned(),
                address: Ipv4Addr::new(192, 168, 1, 5),
                kind: NetworkKind::Wifi,
            },
            port: 39740,
            username: "ket".to_owned(),
            password: "abcdefghjkmn".to_owned(),
        }
    }

    #[test]
    fn the_qr_code_holds_the_proxy_address_with_its_sign_in() {
        let view = share_view(&details());
        assert_eq!(view.proxy_url, "http://ket:abcdefghjkmn@192.168.1.5:39740");
        assert!(view.qr_svg.starts_with("<?xml"));
        assert!(view.qr_svg.contains("<svg"));
    }

    #[test]
    fn refusals_are_named_for_the_ui() {
        assert_eq!(
            share_issue(&ShareError::NetworkUnavailable).code,
            "share_network_unavailable"
        );
    }
}
