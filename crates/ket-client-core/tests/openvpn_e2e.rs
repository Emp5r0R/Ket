use std::{path::PathBuf, time::Duration};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, OpenVpnStunnelAdapter, TransportAdapter,
};
use ket_core::{SessionManifest, TransportProtocol};

#[tokio::test]
async fn openvpn_transport_carries_full_route_traffic_when_configured() {
    let Some(transport_path) = std::env::var_os("KET_OPENVPN_E2E_TRANSPORT") else {
        return;
    };
    let document = std::fs::read(transport_path).expect("read E2E transport");
    let manifest: SessionManifest = serde_json::from_slice(&document).expect("parse E2E manifest");
    let transport = manifest
        .transports
        .into_iter()
        .find(|transport| transport.profile.protocol == TransportProtocol::OpenVpnStunnel)
        .expect("manifest contains OpenVPN over stunnel transport");
    let adapter: Box<dyn TransportAdapter> = if std::env::var_os("KET_OPENVPN_E2E_BROKER").is_some()
    {
        Box::new(BrokerTransportAdapter::new(
            BrokerConfig::from_env().expect("load installed broker configuration"),
        ))
    } else {
        Box::new(OpenVpnStunnelAdapter::new(
            required_path("KET_OPENVPN_E2E_BINARY"),
            required_path("KET_OPENVPN_E2E_STUNNEL"),
            "/tmp/ket-openvpn-e2e",
            "/tmp/ket-openvpn-e2e/resolv.conf.state",
        ))
    };
    let probe = adapter
        .probe(&transport)
        .await
        .expect("probe OpenVPN transport");
    let started = adapter
        .connect(&transport, &probe)
        .await
        .expect("connect OpenVPN transport");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build HTTP client");
    let result = verify_traffic(&client).await;
    let stop = started.tunnel.stop().await;
    result.expect("OpenVPN full-route traffic failed");
    stop.expect("stop OpenVPN tunnel");
}

async fn verify_traffic(client: &reqwest::Client) -> Result<(), String> {
    if let Ok(expected_ip) = std::env::var("KET_OPENVPN_E2E_EGRESS_IP") {
        let trace = fetch(client, "https://www.cloudflare.com/cdn-cgi/trace").await?;
        let observed_ip = trace
            .lines()
            .find_map(|line| line.strip_prefix("ip="))
            .ok_or_else(|| "Cloudflare trace omitted the egress IP".to_owned())?;
        if observed_ip != expected_ip {
            return Err(format!(
                "unexpected tunneled egress IP: expected {expected_ip}, observed {observed_ip}"
            ));
        }
    }

    let targets = std::env::var("KET_OPENVPN_E2E_TARGETS")
        .unwrap_or_else(|_| "https://example.com/".to_owned());
    for target in targets.split(',').filter(|target| !target.is_empty()) {
        let body = fetch(client, target).await?;
        if body.is_empty() {
            return Err(format!("empty tunneled response from {target}"));
        }
    }
    Ok(())
}

async fn fetch(client: &reqwest::Client, target: &str) -> Result<String, String> {
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
    response
        .text()
        .await
        .map_err(|error| format!("read tunneled response from {target}: {error}"))
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is required when KET_OPENVPN_E2E_TRANSPORT is set"))
}
