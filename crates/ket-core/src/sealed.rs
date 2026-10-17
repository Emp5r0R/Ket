//! What travels inside a sealed control message (see the `ket-seal` crate).
//! The outer HTTP request is always `POST /v1/sealed` with an opaque body, so
//! a TLS-terminating front learns neither the route, the credentials, the app
//! version, nor whether the request succeeded.

use serde::{Deserialize, Serialize};

use crate::SecretString;

/// The one public route for sealed control messages.
pub const SEALED_CONTROL_PATH: &str = "/v1/sealed";
/// Media type of sealed request and response bodies.
pub const SEALED_CONTENT_TYPE: &str = "application/octet-stream";
/// How far a sealed request's clock may be from the node's. Outside it the
/// node answers [`SEALED_CLOCK_SKEW_CODE`] with its own time, so the app can
/// correct its offset and retry once.
pub const SEALED_MAX_CLOCK_SKEW_SECONDS: u64 = 300;
/// Error code for a request sent too far from the node's time.
pub const SEALED_CLOCK_SKEW_CODE: &str = "request_clock_skew";
/// Error code for a request the node has already answered.
pub const SEALED_REPLAY_CODE: &str = "request_replayed";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SealedMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl SealedMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

/// A session API request, carried sealed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SealedControlRequest {
    pub method: SealedMethod,
    /// `/v1/sessions` or `/v1/sessions/current`.
    pub path: String,
    pub sent_at_epoch_seconds: u64,
    /// The session token, for `/v1/sessions/current`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<SecretString>,
    /// The `Ket-Client` value, such as `android/1.3.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// The JSON request body, for `POST /v1/sessions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<SecretString>,
}

/// The node's answer to a [`SealedControlRequest`]: the status and JSON body
/// the session API would have sent in the clear.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SealedControlResponse {
    pub status: u16,
    pub server_time_epoch_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<SecretString>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn requests_have_a_stable_wire_form_and_hide_secrets_from_debug() {
        let request = SealedControlRequest {
            method: SealedMethod::Put,
            path: "/v1/sessions/current".to_owned(),
            sent_at_epoch_seconds: 1_800_000_000,
            bearer: Some(SecretString::from("token-secret")),
            client: Some("linux/1.3.0".to_owned()),
            body: None,
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(
            value,
            json!({
                "method": "PUT",
                "path": "/v1/sessions/current",
                "sent_at_epoch_seconds": 1_800_000_000_u64,
                "bearer": "token-secret",
                "client": "linux/1.3.0"
            })
        );
        assert_eq!(
            serde_json::from_value::<SealedControlRequest>(value).unwrap(),
            request
        );
        assert!(!format!("{request:?}").contains("token-secret"));
        for method in [
            SealedMethod::Get,
            SealedMethod::Post,
            SealedMethod::Put,
            SealedMethod::Delete,
        ] {
            assert_eq!(
                serde_json::to_value(method).unwrap(),
                json!(method.as_str())
            );
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(
            serde_json::from_value::<SealedControlRequest>(json!({
                "method": "GET",
                "path": "/v1/sessions/current",
                "sent_at_epoch_seconds": 1,
                "authorization": "x"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SealedControlResponse>(json!({
                "status": 200,
                "server_time_epoch_seconds": 1,
                "headers": {}
            }))
            .is_err()
        );
    }
}
