//! Session API requests sealed to the pinned node key (`POST /v1/sealed`).
//!
//! A front that terminates TLS sees one route and opaque bodies: not the
//! access code, the session token, the credentials the node returns, the app
//! version, or whether a request succeeded. Only the pinned node can open a
//! request, and only the app that sealed it can open the answer, so a sealed
//! answer also proves the node's identity.

use std::sync::atomic::{AtomicI64, Ordering};

use ket_core::{
    ErrorResponse, SEALED_CLOCK_SKEW_CODE, SEALED_CONTENT_TYPE, SealedControlRequest,
    SealedControlResponse, SecretString,
};
use ket_invite::NodePublicKey;
use ket_seal::{MAX_SEALED_BYTES, NodeSealingKey, seal_request};
use reqwest::{Client, StatusCode, Url, header::CONTENT_TYPE};
use zeroize::Zeroizing;

use crate::{
    ClientError,
    api::{ControlRequest, Exchange, api_error, client_header_value, read_bounded},
};

/// Seals requests and corrects for a device clock that disagrees with the
/// node's, which the node reports in every sealed answer.
#[derive(Debug, Default)]
pub(crate) struct SealedControl {
    clock_offset_seconds: AtomicI64,
}

impl SealedControl {
    /// `Ok(None)` when the node has no sealed route (an older server).
    pub(crate) async fn exchange(
        &self,
        client: &Client,
        url: Url,
        node_key: &NodePublicKey,
        request: &ControlRequest<'_>,
    ) -> Result<Option<Exchange>, ClientError> {
        let sealing = NodeSealingKey::from_ed25519(node_key.as_bytes())
            .map_err(|_| ClientError::NodeIdentityMismatch)?;
        let mut answer = self.attempt(client, url.clone(), &sealing, request).await?;
        if let Some(response) = &answer
            && is_clock_skew(response)
        {
            self.clock_offset_seconds.store(
                saturating_offset(response.server_time_epoch_seconds, unix_time()),
                Ordering::Relaxed,
            );
            answer = self.attempt(client, url, &sealing, request).await?;
        }
        Ok(answer.map(|response| Exchange {
            status: StatusCode::from_u16(response.status).unwrap_or(StatusCode::BAD_GATEWAY),
            body: response
                .body
                .map(|body| body.expose_secret().as_bytes().to_vec())
                .unwrap_or_default(),
        }))
    }

    async fn attempt(
        &self,
        client: &Client,
        url: Url,
        sealing: &NodeSealingKey,
        request: &ControlRequest<'_>,
    ) -> Result<Option<SealedControlResponse>, ClientError> {
        let inner = SealedControlRequest {
            method: request.method,
            path: format!("/{}", request.path),
            sent_at_epoch_seconds: self.now(),
            bearer: request.bearer.cloned(),
            client: client_header_value().map(str::to_owned),
            body: request
                .body
                .as_ref()
                .map(|body| SecretString::new(body.as_str())),
        };
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&inner)
                .map_err(|_| ClientError::Local("failed to encode a request".to_owned()))?,
        );
        let (sealed, opener) = seal_request(sealing, &plaintext)
            .map_err(|_| ClientError::Local("failed to seal a request".to_owned()))?;
        let response = client
            .post(url)
            .header(CONTENT_TYPE, SEALED_CONTENT_TYPE)
            .body(sealed)
            .send()
            .await
            .map_err(|error| ClientError::request(&error))?;
        let status = response.status();
        let body = read_bounded(response, MAX_SEALED_BYTES).await?;
        match status {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Ok(None),
            _ => return Err(api_error(status, &body)),
        }
        let plaintext = Zeroizing::new(
            opener
                .open(&body)
                .map_err(|_| ClientError::NodeIdentityMismatch)?,
        );
        serde_json::from_slice(&plaintext)
            .map(Some)
            .map_err(|_| ClientError::InvalidResponse("JSON decoding failed".to_owned()))
    }

    fn now(&self) -> u64 {
        unix_time().saturating_add_signed(self.clock_offset_seconds.load(Ordering::Relaxed))
    }
}

fn is_clock_skew(response: &SealedControlResponse) -> bool {
    response.status == StatusCode::BAD_REQUEST.as_u16()
        && response
            .body
            .as_ref()
            .and_then(|body| serde_json::from_str::<ErrorResponse>(body.expose_secret()).ok())
            .is_some_and(|error| error.code == SEALED_CLOCK_SKEW_CODE)
}

fn saturating_offset(server: u64, local: u64) -> i64 {
    if server >= local {
        i64::try_from(server - local).unwrap_or(i64::MAX)
    } else {
        i64::try_from(local - server).map_or(i64::MIN, |difference| -difference)
    }
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_offsets_are_signed_and_saturate() {
        assert_eq!(saturating_offset(1_100, 1_000), 100);
        assert_eq!(saturating_offset(1_000, 1_100), -100);
        assert_eq!(saturating_offset(u64::MAX, 0), i64::MAX);
        assert_eq!(saturating_offset(0, u64::MAX), i64::MIN);
    }

    #[test]
    fn only_the_clock_skew_code_triggers_a_retry() {
        let response = |status: u16, code: &str| SealedControlResponse {
            status,
            server_time_epoch_seconds: 1,
            body: Some(SecretString::new(
                serde_json::json!({"code": code, "message": "m"}).to_string(),
            )),
        };
        assert!(is_clock_skew(&response(400, SEALED_CLOCK_SKEW_CODE)));
        assert!(!is_clock_skew(&response(400, "invalid_request")));
        assert!(!is_clock_skew(&response(409, SEALED_CLOCK_SKEW_CODE)));
    }
}
