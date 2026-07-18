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
}

fn issue(code: &str, error: &ClientError, retryable: bool) -> ClientIssue {
    ClientIssue {
        code: code.to_owned(),
        message: error.to_string(),
        retryable,
    }
}
