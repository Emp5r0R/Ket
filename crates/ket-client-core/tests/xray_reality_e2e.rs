use std::{path::PathBuf, time::Duration};

use ket_client_core::{TransportAdapter, XrayRealityAdapter};
use ket_core::{SessionManifest, TransportProtocol};

#[tokio::test]
async fn reality_transport_carries_full_route_traffic_when_configured() {
    let Some(transport_path) = std::env::var_os("KET_XRAY_E2E_TRANSPORT") else {
        return;
    };
    let xray = required_path("KET_XRAY_E2E_BINARY");
    let bridge = required_path("KET_XRAY_E2E_BRIDGE");
    let document = std::fs::read(transport_path).expect("read E2E transport");
    let manifest: SessionManifest = serde_json::from_slice(&document).expect("parse E2E manifest");
    let transport = manifest
        .transports
        .into_iter()
        .find(|transport| transport.profile.protocol == TransportProtocol::VlessXtlsReality)
        .expect("manifest contains Reality transport");
    let adapter = XrayRealityAdapter::new(xray, bridge, "/tmp/ket-xray-e2e");
    let probe = adapter
        .probe(&transport)
        .await
        .expect("probe Reality transport");
    let started = adapter
        .connect(&transport, &probe)
        .await
        .expect("connect Reality transport");

    let result = async {
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("build HTTP client")
            .get("https://www.cloudflare.com/cdn-cgi/trace")
            .send()
            .await
            .expect("send traffic through Reality tunnel");
        let status = response.status();
        let body = response.text().await.expect("read tunneled response");
        (status, body)
    }
    .await;
    let stop = started.tunnel.stop().await;
    assert!(result.0.is_success());
    let body = result.1;
    assert!(body.contains("ip=") && body.contains("tls="));
    stop.expect("stop Reality tunnel");
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is required when KET_XRAY_E2E_TRANSPORT is set"))
}
