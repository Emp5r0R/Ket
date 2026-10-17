//! `POST /v1/sealed`: session API requests sealed to the node's pinned key.
//!
//! The gateway opens a request, checks its clock and that it was not answered
//! before, replays it against the same session routes that serve plain
//! requests, and seals the answer. A front that terminates TLS sees one route,
//! an opaque body, and `200` for every request the node could open. Requests
//! it cannot open get the same `404` as any unknown path.

use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use ket_core::{
    CLIENT_HEADER, ErrorResponse, SEALED_CLOCK_SKEW_CODE, SEALED_CONTENT_TYPE,
    SEALED_MAX_CLOCK_SKEW_SECONDS, SEALED_REPLAY_CODE, SealedControlRequest, SealedControlResponse,
    SealedMethod, SecretString,
};
use ket_seal::{NodeOpeningKey, ResponseSealer, open_request};
use tower::ServiceExt as _;

use crate::{invite::InviteIssuer, service::unix_time};

/// Answered requests remembered for replay checks. Old entries go first when
/// it is full, so a flood of fresh requests cannot lock real apps out.
const REPLAY_CAPACITY: usize = 65_536;
/// Upper bound on a session API answer before it is sealed.
const MAX_INNER_RESPONSE_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub(super) struct SealedGateway {
    issuer: Arc<InviteIssuer>,
    sessions: Router,
    replays: Arc<Mutex<ReplayWindow>>,
}

impl SealedGateway {
    /// `sessions` is the session API router with its state applied.
    pub(super) fn new(issuer: Arc<InviteIssuer>, sessions: Router) -> Self {
        Self {
            issuer,
            sessions,
            replays: Arc::new(Mutex::new(ReplayWindow::new(
                REPLAY_CAPACITY,
                2 * SEALED_MAX_CLOCK_SKEW_SECONDS,
            ))),
        }
    }

    pub(super) async fn handle(self, body: Bytes) -> Response {
        let opened = match open_request(self.opening_key(), &body) {
            Ok(opened) => opened,
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        };
        let now = unix_time();
        let answer = match serde_json::from_slice::<SealedControlRequest>(&opened.plaintext) {
            Err(_) => error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "malformed request",
            ),
            Ok(request)
                if request.sent_at_epoch_seconds.abs_diff(now) > SEALED_MAX_CLOCK_SKEW_SECONDS =>
            {
                error(
                    StatusCode::BAD_REQUEST,
                    SEALED_CLOCK_SKEW_CODE,
                    "this device's clock is too far from the server's",
                )
            }
            Ok(request) => {
                let fresh = self
                    .replays
                    .lock()
                    .map(|mut replays| replays.insert(opened.replay_id(), now))
                    .unwrap_or(false);
                if fresh {
                    self.dispatch(request).await
                } else {
                    error(
                        StatusCode::CONFLICT,
                        SEALED_REPLAY_CODE,
                        "this request was already answered",
                    )
                }
            }
        };
        seal_answer(opened.sealer, answer, now)
    }

    fn opening_key(&self) -> &NodeOpeningKey {
        self.issuer.opening_key()
    }

    /// Runs `request` through the session routes, exactly as if it had
    /// arrived in the clear without the outer request's headers.
    async fn dispatch(&self, request: SealedControlRequest) -> Answer {
        let Some(inner) = inner_request(request) else {
            return error(StatusCode::NOT_FOUND, "not_found", "not found");
        };
        let response = match self.sessions.clone().oneshot(inner).await {
            Ok(response) => response,
            Err(infallible) => match infallible {},
        };
        let status = response.status();
        match to_bytes(response.into_body(), MAX_INNER_RESPONSE_BYTES).await {
            Ok(body) => match String::from_utf8(body.to_vec()) {
                Ok(text) => Answer {
                    status,
                    body: (!text.is_empty()).then(|| SecretString::new(text)),
                },
                Err(_) => internal(),
            },
            Err(_) => internal(),
        }
    }
}

struct Answer {
    status: StatusCode,
    body: Option<SecretString>,
}

fn error(status: StatusCode, code: &str, message: &str) -> Answer {
    let body = serde_json::to_string(&ErrorResponse {
        code: code.to_owned(),
        message: message.to_owned(),
        update: None,
    })
    .ok()
    .map(SecretString::new);
    Answer { status, body }
}

fn internal() -> Answer {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "internal server error",
    )
}

/// Only the session routes are reachable through the gateway.
fn inner_request(request: SealedControlRequest) -> Option<Request<Body>> {
    let method = match (request.path.as_str(), request.method) {
        ("/v1/sessions", SealedMethod::Post) => Method::POST,
        ("/v1/sessions/current", SealedMethod::Get) => Method::GET,
        ("/v1/sessions/current", SealedMethod::Put) => Method::PUT,
        ("/v1/sessions/current", SealedMethod::Delete) => Method::DELETE,
        _ => return None,
    };
    let mut builder = Request::builder().method(method).uri(request.path.as_str());
    if let Some(bearer) = &request.bearer {
        let value = HeaderValue::from_str(&format!("Bearer {}", bearer.expose_secret())).ok()?;
        builder = builder.header(header::AUTHORIZATION, value);
    }
    if let Some(client) = &request.client {
        builder = builder.header(CLIENT_HEADER, HeaderValue::from_str(client).ok()?);
    }
    let body = match &request.body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.expose_secret().to_owned())
        }
        None => Body::empty(),
    };
    builder.body(body).ok()
}

fn seal_answer(sealer: ResponseSealer, answer: Answer, now: u64) -> Response {
    let response = SealedControlResponse {
        status: answer.status.as_u16(),
        server_time_epoch_seconds: now,
        body: answer.body,
    };
    let sealed = serde_json::to_vec(&response)
        .ok()
        .map(zeroize::Zeroizing::new)
        .and_then(|plaintext| sealer.seal(&plaintext).ok());
    match sealed {
        Some(sealed) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static(SEALED_CONTENT_TYPE),
            )],
            sealed,
        )
            .into_response(),
        None => {
            tracing::error!("failed to seal a control response");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Replay IDs of answered requests, each kept for `window_seconds`.
struct ReplayWindow {
    capacity: usize,
    window_seconds: u64,
    seen: HashSet<[u8; 32]>,
    order: VecDeque<([u8; 32], u64)>,
}

impl ReplayWindow {
    fn new(capacity: usize, window_seconds: u64) -> Self {
        Self {
            capacity,
            window_seconds,
            seen: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    /// False when `id` was already answered within the window.
    fn insert(&mut self, id: [u8; 32], now: u64) -> bool {
        while let Some(&(old, at)) = self.order.front() {
            if now.saturating_sub(at) <= self.window_seconds {
                break;
            }
            self.order.pop_front();
            self.seen.remove(&old);
        }
        if self.seen.contains(&id) {
            return false;
        }
        if self.order.len() >= self.capacity
            && let Some((oldest, _)) = self.order.pop_front()
        {
            self.seen.remove(&oldest);
        }
        self.seen.insert(id);
        self.order.push_back((id, now));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_window_refuses_repeats_until_they_age_out() {
        let mut window = ReplayWindow::new(3, 600);
        assert!(window.insert([1; 32], 1_000));
        assert!(!window.insert([1; 32], 1_100));
        assert!(window.insert([2; 32], 1_100));
        // Aged out after the window.
        assert!(window.insert([1; 32], 1_601));
        assert!(!window.insert([2; 32], 1_601));
    }

    #[test]
    fn a_full_replay_window_drops_its_oldest_entry() {
        let mut window = ReplayWindow::new(2, 600);
        assert!(window.insert([1; 32], 1_000));
        assert!(window.insert([2; 32], 1_000));
        assert!(window.insert([3; 32], 1_000));
        assert_eq!(window.order.len(), 2);
        assert!(window.insert([1; 32], 1_000));
        assert!(!window.insert([3; 32], 1_000));
    }

    #[test]
    fn only_session_routes_are_reachable() {
        let request = |method, path: &str| SealedControlRequest {
            method,
            path: path.to_owned(),
            sent_at_epoch_seconds: 0,
            bearer: None,
            client: None,
            body: None,
        };
        for (method, path) in [
            (SealedMethod::Post, "/v1/sessions"),
            (SealedMethod::Get, "/v1/sessions/current"),
            (SealedMethod::Put, "/v1/sessions/current"),
            (SealedMethod::Delete, "/v1/sessions/current"),
        ] {
            assert!(inner_request(request(method, path)).is_some(), "{path}");
        }
        for (method, path) in [
            (SealedMethod::Get, "/v1/sessions"),
            (SealedMethod::Post, "/v1/sessions/current"),
            (SealedMethod::Get, "/v1/admin/access-grants"),
            (SealedMethod::Get, "/metrics"),
            (SealedMethod::Post, "/internal/v1/openvpn/auth"),
            (SealedMethod::Get, "/v1/sessions/current/../../metrics"),
        ] {
            assert!(inner_request(request(method, path)).is_none(), "{path}");
        }
    }

    #[test]
    fn header_injection_is_refused() {
        let mut request = SealedControlRequest {
            method: SealedMethod::Get,
            path: "/v1/sessions/current".to_owned(),
            sent_at_epoch_seconds: 0,
            bearer: Some(SecretString::from("token\r\nX-Admin: 1")),
            client: None,
            body: None,
        };
        assert!(inner_request(request.clone()).is_none());
        request.bearer = None;
        request.client = Some("linux/1.0.0\r\nX: y".to_owned());
        assert!(inner_request(request).is_none());
    }
}
