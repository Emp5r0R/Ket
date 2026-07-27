use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientIssue {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("server URL is invalid: {0}")]
    InvalidEndpoint(String),
    #[error("input is invalid: {0}")]
    InvalidInput(String),
    #[error("the client has not been enrolled")]
    NotEnrolled,
    #[error("a tunnel is already connected")]
    AlreadyConnected,
    #[error("the control server rejected the request ({code})")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("the control server could not be reached: {0}")]
    Network(&'static str),
    #[error("the control server response exceeded the size limit")]
    ResponseTooLarge,
    #[error("the control server returned invalid data: {0}")]
    InvalidResponse(String),
    #[error("no compatible transport is available")]
    NoCompatibleTransport,
    #[error("transport {transport_id} failed: {message}")]
    DnsResolution {
        transport_id: String,
        message: &'static str,
    },
    #[error("transport {transport_id} failed: {message}")]
    Transport {
        transport_id: String,
        message: String,
        retryable: bool,
    },
    #[error("local client operation failed: {0}")]
    Local(String),
}

impl ClientError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Api { status: 401, .. })
    }

    pub fn issue(&self) -> ClientIssue {
        match self {
            Self::InvalidEndpoint(_) => issue("invalid_endpoint", self, false),
            Self::InvalidInput(_) => issue("invalid_input", self, false),
            Self::NotEnrolled => issue("not_enrolled", self, false),
            Self::AlreadyConnected => issue("already_connected", self, false),
            Self::Api {
                status,
                code,
                message,
            } => ClientIssue {
                code: code.clone(),
                message: message.clone(),
                retryable: *status >= 500 || *status == 429,
            },
            Self::Network(_) => issue("network_unavailable", self, true),
            Self::ResponseTooLarge => issue("response_too_large", self, false),
            Self::InvalidResponse(_) => issue("invalid_server_response", self, false),
            Self::NoCompatibleTransport => issue("no_compatible_transport", self, false),
            Self::DnsResolution { .. } => issue("transport_dns_failed", self, true),
            Self::Transport { retryable, .. } => ClientIssue {
                code: "transport_failed".to_owned(),
                message: self.to_string(),
                retryable: *retryable,
            },
            Self::Local(_) => issue("local_failure", self, false),
        }
    }

    pub(crate) fn request(error: &reqwest::Error) -> Self {
        let message = if error.is_timeout() {
            "request timed out"
        } else if error.is_connect() {
            "connection failed"
        } else if error.is_body() {
            "response body failed"
        } else {
            "request failed"
        };
        Self::Network(message)
    }

    pub(crate) fn transport(
        transport_id: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::Transport {
            transport_id: transport_id.into(),
            message: message.into(),
            retryable,
        }
    }

    pub(crate) fn dns_resolution(transport_id: impl Into<String>, message: &'static str) -> Self {
        Self::DnsResolution {
            transport_id: transport_id.into(),
            message,
        }
    }

    pub(crate) fn is_dns_resolution_failure(&self) -> bool {
        matches!(self, Self::DnsResolution { .. })
    }
}

fn issue(code: &str, error: &ClientError, retryable: bool) -> ClientIssue {
    ClientIssue {
        code: code.to_owned(),
        message: error.to_string(),
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_variant_has_a_stable_public_issue_policy() {
        let cases = [
            (
                ClientError::InvalidEndpoint("bad".to_owned()),
                "invalid_endpoint",
                false,
            ),
            (
                ClientError::InvalidInput("bad".to_owned()),
                "invalid_input",
                false,
            ),
            (ClientError::NotEnrolled, "not_enrolled", false),
            (ClientError::AlreadyConnected, "already_connected", false),
            (
                ClientError::Network("request timed out"),
                "network_unavailable",
                true,
            ),
            (ClientError::ResponseTooLarge, "response_too_large", false),
            (
                ClientError::InvalidResponse("bad JSON".to_owned()),
                "invalid_server_response",
                false,
            ),
            (
                ClientError::NoCompatibleTransport,
                "no_compatible_transport",
                false,
            ),
            (
                ClientError::dns_resolution("reality", "DNS timed out"),
                "transport_dns_failed",
                true,
            ),
            (
                ClientError::transport("reality", "blocked", true),
                "transport_failed",
                true,
            ),
            (
                ClientError::transport("reality", "invalid config", false),
                "transport_failed",
                false,
            ),
            (
                ClientError::Local("disk failed".to_owned()),
                "local_failure",
                false,
            ),
        ];

        for (error, code, retryable) in cases {
            let issue = error.issue();
            assert_eq!(issue.code, code, "wrong code for {error:?}");
            assert_eq!(
                issue.retryable, retryable,
                "wrong retry policy for {error:?}"
            );
            assert!(!issue.message.is_empty());
        }
    }

    #[test]
    fn api_errors_preserve_safe_server_copy_and_retry_only_transient_statuses() {
        for (status, retryable) in [
            (400, false),
            (401, false),
            (403, false),
            (429, true),
            (500, true),
        ] {
            let error = ClientError::Api {
                status,
                code: "server_code".to_owned(),
                message: "bounded server message".to_owned(),
            };
            assert_eq!(error.is_unauthorized(), status == 401);
            assert_eq!(
                error.issue(),
                ClientIssue {
                    code: "server_code".to_owned(),
                    message: "bounded server message".to_owned(),
                    retryable,
                }
            );
        }
    }

    #[test]
    fn dns_failures_are_distinguishable_from_other_transport_failures() {
        assert!(ClientError::dns_resolution("x", "DNS timed out").is_dns_resolution_failure());
        assert!(!ClientError::transport("x", "failed", true).is_dns_resolution_failure());
    }
}
