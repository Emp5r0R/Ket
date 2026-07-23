use std::{path::PathBuf, time::Duration};

use ket_client_core::{TransportAdapter, XrayAdapter};
use ket_core::{SessionManifest, TransportProtocol};

#[tokio::test]
async fn xray_transport_carries_full_route_traffic_when_configured() {
    let Some(transport_path) = std::env::var_os("KET_XRAY_E2E_TRANSPORT") else {
        return;
    };
    let xray = required_path("KET_XRAY_E2E_BINARY");
    let bridge = required_path("KET_XRAY_E2E_BRIDGE");
    let document = std::fs::read(transport_path).expect("read E2E transport");
    let manifest: SessionManifest = serde_json::from_slice(&document).expect("parse E2E manifest");
    let protocol = match std::env::var("KET_XRAY_E2E_PROTOCOL").as_deref() {
        Ok("stealth") => TransportProtocol::Stealth,
        Ok("reality") | Err(_) => TransportProtocol::VlessXtlsReality,
        Ok(value) => panic!("unsupported KET_XRAY_E2E_PROTOCOL: {value}"),
    };
    let transport = manifest
        .transports
        .into_iter()
        .find(|transport| transport.profile.protocol == protocol)
        .expect("manifest contains requested Xray transport");
    let adapter = XrayAdapter::new(
        xray,
        bridge,
        "/tmp/ket-xray-e2e",
        "/tmp/ket-xray-e2e/resolv.conf.state",
    );
    let probe = adapter
        .probe(&transport)
        .await
        .expect("probe Reality transport");
    let started = adapter
        .connect(&transport, &probe)
        .await
        .expect("connect Reality transport");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build HTTP client");
    let targets = std::env::var("KET_XRAY_E2E_TARGETS")
        .unwrap_or_else(|_| "https://www.cloudflare.com/cdn-cgi/trace".to_owned());
    let result: Result<(), String> = async {
        for target in targets.split(',').filter(|target| !target.is_empty()) {
            let response = client
                .get(target)
                .send()
                .await
                .map_err(|error| format!("send tunneled request to {target}: {error}"))?;
            if !response.status().is_success() && !response.status().is_client_error() {
                return Err(format!(
                    "unexpected tunneled status for {target}: {}",
                    response.status()
                ));
            }
            let body = response
                .bytes()
                .await
                .map_err(|error| format!("read tunneled response from {target}: {error}"))?;
            if body.is_empty() {
                return Err(format!("empty tunneled response from {target}"));
            }
        }
        Ok(())
    }
    .await;
    let stop = started.tunnel.stop().await;
    result.expect("Xray full-route traffic failed");
    stop.expect("stop Xray tunnel");
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is required when KET_XRAY_E2E_TRANSPORT is set"))
}
