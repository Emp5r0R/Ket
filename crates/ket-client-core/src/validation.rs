use std::collections::BTreeSet;

use ket_core::{
    NodeStatus, SecretString, SessionManifest, SessionStatus, SessionTraffic, TransportProtocol,
    split_session_token,
};
use reqwest::Url;

use crate::ClientError;

const MAX_NODE_TEXT_CHARS: usize = 128;
const MAX_PUBLIC_URL_CHARS: usize = 2_048;
const MAX_TRANSPORTS: usize = 32;
const MAX_TRANSPORT_ID_CHARS: usize = 128;
const MAX_ENDPOINT_CHARS: usize = 253;
const MAX_OPTION_ENTRIES: usize = 32;
const MAX_OPTION_KEY_CHARS: usize = 64;
const MAX_OPTION_VALUE_CHARS: usize = 2_048;
const MAX_SECRET_ENTRIES: usize = 16;
const MAX_SECRET_CHARS: usize = 4_096;
const MAX_OPENVPN_ENCODED_MATERIAL_CHARS: usize = (8 * 1024_usize).div_ceil(3) * 4;

/// Validates the complete untrusted enrollment document before it enters runtime state.
pub(crate) fn validate_manifest(
    manifest: &SessionManifest,
    now_epoch_seconds: u64,
) -> Result<(), ClientError> {
    split_session_token(manifest.session_token.expose_secret())
        .map_err(|_| invalid("session token has an invalid shape"))?;
    validate_future_expiry(manifest.session_expires_at_epoch_seconds, now_epoch_seconds)?;
    validate_node(&manifest.node)?;
    if manifest.transports.is_empty() || manifest.transports.len() > MAX_TRANSPORTS {
        return Err(invalid("server advertised an invalid transport count"));
    }

    let mut ids = BTreeSet::new();
    for transport in &manifest.transports {
        let profile = &transport.profile;
        validate_identifier(&profile.id, "transport ID", MAX_TRANSPORT_ID_CHARS)?;
        validate_text(
            &profile.display_name,
            "transport display name",
            MAX_NODE_TEXT_CHARS,
        )?;
        validate_endpoint(&profile.endpoint)?;
        if profile.port == 0 || !ids.insert(profile.id.as_str()) {
            return Err(invalid("server advertised an invalid transport profile"));
        }
        if let Some(server_name) = &profile.tls_server_name {
            validate_endpoint_name(server_name, "TLS server name")?;
        }
        if profile.options.len() > MAX_OPTION_ENTRIES {
            return Err(invalid("transport profile contains too many options"));
        }
        for (key, value) in &profile.options {
            validate_option_key(key, "transport option")?;
            validate_bounded_value(value, "transport option value", MAX_OPTION_VALUE_CHARS)?;
        }
        if let Some(credential) = &transport.credential {
            validate_secret(credential.auth.expose_secret(), "transport credential")?;
            if credential.secrets.len() > MAX_SECRET_ENTRIES {
                return Err(invalid("transport credential contains too many secrets"));
            }
            for (key, value) in &credential.secrets {
                validate_option_key(key, "transport secret")?;
                let maximum_chars = if profile.protocol == TransportProtocol::OpenVpnStunnel
                    && matches!(
                        key.as_str(),
                        "ca_certificate_pem_b64"
                            | "stunnel_ca_certificate_pem_b64"
                            | "tls_crypt_key_b64"
                    ) {
                    MAX_OPENVPN_ENCODED_MATERIAL_CHARS
                } else {
                    MAX_SECRET_CHARS
                };
                validate_bounded_secret(
                    value.expose_secret(),
                    "transport secret value",
                    maximum_chars,
                )?;
            }
        }
    }
    Ok(())
}

/// Binds refreshed telemetry to the active lease before replacing the last known-good snapshot.
pub(crate) fn validate_status(
    status: &SessionStatus,
    token: &SecretString,
    expected_client_name: &str,
    now_epoch_seconds: u64,
) -> Result<(), ClientError> {
    let token = split_session_token(token.expose_secret())
        .map_err(|_| invalid("saved session token has an invalid shape"))?;
    if status.session_id != token.id {
        return Err(invalid("session status identity does not match enrollment"));
    }
    validate_text(&status.client_name, "session client name", 96)?;
    if status.client_name != expected_client_name {
        return Err(invalid(
            "session status client name does not match enrollment",
        ));
    }
    validate_future_expiry(status.expires_at_epoch_seconds, now_epoch_seconds)?;
    validate_node(&status.node)?;
    validate_traffic(&status.traffic)
}

fn validate_node(node: &NodeStatus) -> Result<(), ClientError> {
    validate_identifier(&node.node_id, "node ID", MAX_NODE_TEXT_CHARS)?;
    validate_text(&node.display_name, "node display name", MAX_NODE_TEXT_CHARS)?;
    validate_public_url(&node.public_url)?;

    let location = &node.location;
    if location.country_code.len() != 2
        || !location
            .country_code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase())
    {
        return Err(invalid("node country code is invalid"));
    }
    validate_text(
        &location.country_name,
        "node country name",
        MAX_NODE_TEXT_CHARS,
    )?;
    if let Some(city) = &location.city {
        validate_text(city, "node city", MAX_NODE_TEXT_CHARS)?;
    }
    if !location.latitude.is_finite() || !(-90.0..=90.0).contains(&location.latitude) {
        return Err(invalid("node latitude is invalid"));
    }
    if !location.longitude.is_finite() || !(-180.0..=180.0).contains(&location.longitude) {
        return Err(invalid("node longitude is invalid"));
    }
    if node.session_capacity == 0 || node.active_sessions > node.session_capacity {
        return Err(invalid("node session capacity is invalid"));
    }
    if !node.capacity_percent.is_finite() || !(0.0..=100.0).contains(&node.capacity_percent) {
        return Err(invalid("node capacity percent is invalid"));
    }
    if node
        .cpu_load_percent
        .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
    {
        return Err(invalid("node CPU load is invalid"));
    }
    match (node.memory_used_bytes, node.memory_total_bytes) {
        (None, None) => {}
        (Some(used), Some(total)) if total > 0 && used <= total => {}
        _ => return Err(invalid("node memory telemetry is invalid")),
    }
    if node.observed_at_epoch_seconds == 0 {
        return Err(invalid("node observation time is invalid"));
    }
    Ok(())
}

fn validate_traffic(traffic: &SessionTraffic) -> Result<(), ClientError> {
    if traffic.observed_at_epoch_seconds == 0 {
        return Err(invalid("traffic observation time is invalid"));
    }
    if !traffic.available
        && (traffic.bytes_sent != 0
            || traffic.bytes_received != 0
            || traffic.online_connections != 0)
    {
        return Err(invalid("unavailable traffic telemetry is inconsistent"));
    }
    Ok(())
}

fn validate_future_expiry(expires_at: u64, now: u64) -> Result<(), ClientError> {
    if expires_at <= now {
        Err(invalid("session lease is already expired"))
    } else {
        Ok(())
    }
}

fn validate_public_url(value: &str) -> Result<(), ClientError> {
    validate_text(value, "node public URL", MAX_PUBLIC_URL_CHARS)?;
    let url = Url::parse(value).map_err(|_| invalid("node public URL is invalid"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("node public URL is invalid"));
    }
    Ok(())
}

fn validate_endpoint(value: &str) -> Result<(), ClientError> {
    validate_endpoint_name(value, "transport endpoint")?;
    if value.contains("://") || value.contains('/') || value.contains('?') || value.contains('#') {
        return Err(invalid("transport endpoint is invalid"));
    }
    Ok(())
}

fn validate_endpoint_name(value: &str, label: &'static str) -> Result<(), ClientError> {
    if value.is_empty()
        || value.len() > MAX_ENDPOINT_CHARS
        || value != value.trim()
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(invalid(label));
    }
    Ok(())
}

fn validate_identifier(
    value: &str,
    label: &'static str,
    maximum_chars: usize,
) -> Result<(), ClientError> {
    if value.is_empty()
        || value.chars().count() > maximum_chars
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(label));
    }
    Ok(())
}

fn validate_text(
    value: &str,
    label: &'static str,
    maximum_chars: usize,
) -> Result<(), ClientError> {
    if value.is_empty()
        || value != value.trim()
        || value.chars().count() > maximum_chars
        || value.chars().any(char::is_control)
    {
        return Err(invalid(label));
    }
    Ok(())
}

fn validate_option_key(value: &str, label: &'static str) -> Result<(), ClientError> {
    if value.is_empty()
        || value.len() > MAX_OPTION_KEY_CHARS
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(invalid(label));
    }
    Ok(())
}

fn validate_bounded_value(
    value: &str,
    label: &'static str,
    maximum_chars: usize,
) -> Result<(), ClientError> {
    if value.chars().count() > maximum_chars || value.chars().any(char::is_control) {
        return Err(invalid(label));
    }
    Ok(())
}

fn validate_secret(value: &str, label: &'static str) -> Result<(), ClientError> {
    validate_bounded_secret(value, label, MAX_SECRET_CHARS)
}

fn validate_bounded_secret(
    value: &str,
    label: &'static str,
    maximum_chars: usize,
) -> Result<(), ClientError> {
    if value.is_empty()
        || value.chars().count() > maximum_chars
        || value.chars().any(char::is_control)
    {
        return Err(invalid(label));
    }
    Ok(())
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::InvalidResponse(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ket_core::{
        HealthState, Network, NodeLocation, SessionTransport, TransportCredential,
        TransportProfile, TransportProtocol,
    };

    use super::*;

    const NOW: u64 = 4_000_000_000;
    const TOKEN: &str = "A23456789012B3456789012345678901234567890123";

    #[test]
    fn accepts_a_complete_bounded_contract() {
        let manifest = manifest();
        let status = status();

        validate_manifest(&manifest, NOW).unwrap();
        validate_status(&status, &manifest.session_token, "Desktop", NOW).unwrap();
    }

    #[test]
    fn rejects_invalid_geography_and_host_telemetry() {
        let mut response = manifest();
        response.node.location.latitude = 91.0;
        assert_invalid(validate_manifest(&response, NOW), "latitude");

        let mut response = manifest();
        response.node.active_sessions = 11;
        assert_invalid(validate_manifest(&response, NOW), "capacity");

        let mut response = manifest();
        response.node.memory_total_bytes = None;
        assert_invalid(validate_manifest(&response, NOW), "memory");

        let mut response = manifest();
        response.node.cpu_load_percent = Some(f32::NAN);
        assert_invalid(validate_manifest(&response, NOW), "CPU");
    }

    #[test]
    fn rejects_expired_or_ambiguous_manifests() {
        let mut response = manifest();
        response.session_expires_at_epoch_seconds = NOW;
        assert_invalid(validate_manifest(&response, NOW), "expired");

        let mut response = manifest();
        response.transports.push(response.transports[0].clone());
        assert_invalid(validate_manifest(&response, NOW), "transport profile");

        let mut response = manifest();
        response.transports[0].profile.endpoint = "vpn.example.test/path".to_owned();
        assert_invalid(validate_manifest(&response, NOW), "endpoint");
    }

    #[test]
    fn permits_large_bounded_openvpn_material_only_for_known_fields() {
        let mut response = manifest();
        response.transports[0].profile.protocol = TransportProtocol::OpenVpnStunnel;
        response.transports[0]
            .credential
            .as_mut()
            .unwrap()
            .secrets
            .insert(
                "stunnel_ca_certificate_pem_b64".to_owned(),
                "A".repeat(10_000).into(),
            );
        validate_manifest(&response, NOW).unwrap();

        response.transports[0]
            .credential
            .as_mut()
            .unwrap()
            .secrets
            .insert("unexpected_secret".to_owned(), "A".repeat(10_000).into());
        assert_invalid(validate_manifest(&response, NOW), "transport secret value");
    }

    #[test]
    fn binds_status_to_the_enrolled_identity() {
        let manifest = manifest();
        let mut response = status();
        response.session_id = "Z23456789012".to_owned();
        assert_invalid(
            validate_status(&response, &manifest.session_token, "Desktop", NOW),
            "identity",
        );

        let mut response = status();
        response.client_name = "Different device".to_owned();
        assert_invalid(
            validate_status(&response, &manifest.session_token, "Desktop", NOW),
            "client name",
        );
    }

    #[test]
    fn rejects_inconsistent_traffic_telemetry() {
        let manifest = manifest();
        let mut response = status();
        response.traffic.available = false;
        assert_invalid(
            validate_status(&response, &manifest.session_token, "Desktop", NOW),
            "inconsistent",
        );

        let mut response = status();
        response.traffic.observed_at_epoch_seconds = 0;
        assert_invalid(
            validate_status(&response, &manifest.session_token, "Desktop", NOW),
            "observation",
        );
    }

    fn manifest() -> SessionManifest {
        SessionManifest {
            session_token: TOKEN.into(),
            session_expires_at_epoch_seconds: NOW + 300,
            node: node(),
            transports: vec![SessionTransport {
                profile: TransportProfile {
                    id: "hy2-primary".to_owned(),
                    display_name: "Hysteria 2".to_owned(),
                    protocol: TransportProtocol::Hysteria2,
                    endpoint: "vpn.example.test".to_owned(),
                    port: 443,
                    network: Network::Udp,
                    priority: 1,
                    tls_server_name: Some("vpn.example.test".to_owned()),
                    options: BTreeMap::new(),
                },
                credential: Some(TransportCredential {
                    auth: "scoped-authentication-value".into(),
                    secrets: BTreeMap::new(),
                }),
            }],
        }
    }

    fn status() -> SessionStatus {
        SessionStatus {
            session_id: "A23456789012".to_owned(),
            client_name: "Desktop".to_owned(),
            expires_at_epoch_seconds: NOW + 300,
            node: node(),
            traffic: SessionTraffic {
                available: true,
                bytes_sent: 100,
                bytes_received: 200,
                online_connections: 1,
                observed_at_epoch_seconds: NOW,
            },
        }
    }

    fn node() -> NodeStatus {
        NodeStatus {
            node_id: "node-test-1".to_owned(),
            display_name: "Test node".to_owned(),
            public_url: "https://node.example.test/ket".to_owned(),
            location: NodeLocation {
                country_code: "DE".to_owned(),
                country_name: "Germany".to_owned(),
                city: Some("Frankfurt".to_owned()),
                latitude: 50.1109,
                longitude: 8.6821,
            },
            health: HealthState::Healthy,
            active_sessions: 1,
            session_capacity: 10,
            capacity_percent: 10.0,
            cpu_load_percent: Some(20.0),
            memory_used_bytes: Some(1_024),
            memory_total_bytes: Some(4_096),
            uptime_seconds: Some(60),
            observed_at_epoch_seconds: NOW,
        }
    }

    fn assert_invalid(result: Result<(), ClientError>, message_fragment: &str) {
        let error = result.expect_err("contract must be rejected");
        assert!(
            error.to_string().contains(message_fragment),
            "unexpected error: {error}"
        );
    }
}
