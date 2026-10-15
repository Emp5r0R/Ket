//! Wire contract for update advice: which Ket version a server runs and what
//! it asks of the connecting app. The server evaluates the policy (see the
//! `ket-update` crate); apps only display and enforce the result.

use serde::{Deserialize, Serialize};

/// Request header in which apps name their platform and version, for example
/// `Ket-Client: android/1.2.0`. Apps that send none are treated as v1.0.1,
/// the last release before the header existed.
pub const CLIENT_HEADER: &str = "Ket-Client";

/// Error code of the `426 Upgrade Required` response that refuses an app
/// below the server's required version.
pub const CLIENT_UPDATE_REQUIRED_CODE: &str = "client_update_required";

/// How strongly the server asks this app to update, from least to most urgent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientUpdateStatus {
    /// The app is as new as the server knows about.
    Current,
    /// A newer app exists; nothing it fixes affects this app.
    Optional,
    /// A newer app fixes bugs worth having.
    Recommended,
    /// A newer app fixes a security problem. The app keeps working until
    /// `required_after_epoch_seconds`, then must update.
    Security,
    /// The server no longer accepts this app version.
    Required,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientUpdate {
    pub status: ClientUpdateStatus,
    /// The newest app version this server knows about.
    pub latest_version: String,
    /// For `security`: when the update becomes required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_after_epoch_seconds: Option<u64>,
    /// One sentence on the most important change, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Where to get the update (the operator may point at a mirror).
    pub download_url: String,
}

/// Update advice attached to authenticated session responses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateAdvice {
    pub server_version: String,
    pub client: ClientUpdate,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn status_wire_names_are_stable_and_ordered_by_urgency() {
        let statuses = [
            (ClientUpdateStatus::Current, "current"),
            (ClientUpdateStatus::Optional, "optional"),
            (ClientUpdateStatus::Recommended, "recommended"),
            (ClientUpdateStatus::Security, "security"),
            (ClientUpdateStatus::Required, "required"),
        ];
        for pair in statuses.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for (status, name) in statuses {
            assert_eq!(serde_json::to_value(status).unwrap(), json!(name));
        }
    }

    #[test]
    fn advice_omits_absent_optional_fields() {
        let advice = UpdateAdvice {
            server_version: "1.2.0".to_owned(),
            client: ClientUpdate {
                status: ClientUpdateStatus::Current,
                latest_version: "1.2.0".to_owned(),
                required_after_epoch_seconds: None,
                summary: None,
                download_url: "https://example.test/ket".to_owned(),
            },
        };
        assert_eq!(
            serde_json::to_value(&advice).unwrap(),
            json!({
                "server_version": "1.2.0",
                "client": {
                    "status": "current",
                    "latest_version": "1.2.0",
                    "download_url": "https://example.test/ket"
                }
            })
        );
    }
}
