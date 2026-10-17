//! Sealed control messages: an app encrypts each control request to the
//! node's pinned Ed25519 identity, and the node encrypts its response to that
//! request, so a TLS-terminating front (a CDN or tunnel) relays only
//! ciphertext.
//!
//! Requests use HPKE (RFC 9180) base mode with DHKEM(X25519, HKDF-SHA256),
//! HKDF-SHA256, and ChaCha20-Poly1305. The node's X25519 key is the
//! birational image of its Ed25519 key, so every existing invite already
//! carries it (see <https://eprint.iacr.org/2021/509> for the joint security
//! of the two uses). Responses follow Oblivious HTTP (RFC 9458, section 4.4):
//! a fresh response nonce and a secret exported from the request's context.
//!
//! The crate only seals bytes; what the plaintext means is the caller's.

mod keys;
mod request;
mod response;

pub use keys::{NodeOpeningKey, NodeSealingKey};
pub use request::{RequestOpened, open_request, seal_request};
pub use response::{RESPONSE_SECRET_BYTES, ResponseOpener, ResponseSealer};

/// First byte of every sealed request; a new value means a new construction.
pub const SEAL_VERSION: u8 = 1;
/// Upper bound on a sealed message, request or response.
pub const MAX_SEALED_BYTES: usize = 256 * 1024;

/// Every failure is the same to a caller: the message is not a valid sealed
/// message for this key. Details would only help an attacker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SealError;

impl std::fmt::Display for SealError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not a valid sealed message")
    }
}

impl std::error::Error for SealError {}

type Kem = hpke::kem::X25519HkdfSha256;
type Kdf = hpke::kdf::HkdfSha256;
type Aead = hpke::aead::ChaCha20Poly1305;

const ENCAPSULATED_KEY_BYTES: usize = 32;
const INFO_DOMAIN: &[u8] = b"KET-SEAL-V1 request";
const RESPONSE_EXPORT_LABEL: &[u8] = b"KET-SEAL-V1 response";

/// HPKE `info`: the domain, then the node's Ed25519 key, so a request is
/// bound to the node it was sealed for.
fn info(node_public_key: &[u8; 32]) -> Vec<u8> {
    let mut info = Vec::with_capacity(INFO_DOMAIN.len() + 32);
    info.extend_from_slice(INFO_DOMAIN);
    info.extend_from_slice(node_public_key);
    info
}

#[cfg(test)]
mod tests;
