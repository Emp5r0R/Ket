//! Node-authenticated enrollment: the client sends a fresh nonce and the node
//! signs the exact response body it returns for that nonce.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};

use crate::{InviteError, NodePublicKey, NodeSigningKey};

pub const ENROLLMENT_NONCE_HEADER: &str = "ket-enrollment-nonce";
pub const NODE_SIGNATURE_HEADER: &str = "ket-node-signature";
pub const ENROLLMENT_NONCE_BYTES: usize = 32;

const SIGNATURE_DOMAIN: &[u8] = b"KET-ENROLLMENT-V1";
const SIGNATURE_BYTES: usize = 64;

/// A single-use 32-byte enrollment challenge.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct EnrollmentNonce([u8; ENROLLMENT_NONCE_BYTES]);

impl EnrollmentNonce {
    pub fn generate() -> Self {
        let mut nonce = [0_u8; ENROLLMENT_NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce);
        Self(nonce)
    }

    pub fn from_bytes(bytes: [u8; ENROLLMENT_NONCE_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn header_value(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_header_value(value: &str) -> Result<Self, InviteError> {
        decode_exact::<ENROLLMENT_NONCE_BYTES>(value).map(Self)
    }
}

impl std::fmt::Debug for EnrollmentNonce {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EnrollmentNonce(..)")
    }
}

/// Signs and verifies `Ket-Node-Signature` over an enrollment response body.
pub struct EnrollmentProof;

impl EnrollmentProof {
    pub fn sign(signing_key: &NodeSigningKey, nonce: &EnrollmentNonce, body: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(signing_key.sign(&signature_input(nonce, body)))
    }

    /// Fails with [`InviteError::NodeIdentityMismatch`] unless `header_value`
    /// is the pinned node's signature over this nonce and exact body.
    pub fn verify(
        node_public_key: &NodePublicKey,
        nonce: &EnrollmentNonce,
        body: &[u8],
        header_value: Option<&str>,
    ) -> Result<(), InviteError> {
        let signature = header_value
            .and_then(|value| decode_exact::<SIGNATURE_BYTES>(value).ok())
            .ok_or(InviteError::NodeIdentityMismatch)?;
        node_public_key
            .verify(&signature_input(nonce, body), &signature)
            .then_some(())
            .ok_or(InviteError::NodeIdentityMismatch)
    }
}

/// `domain || nonce || u32be(len(body)) || body`.
fn signature_input(nonce: &EnrollmentNonce, body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).unwrap_or(u32::MAX);
    let mut input =
        Vec::with_capacity(SIGNATURE_DOMAIN.len() + ENROLLMENT_NONCE_BYTES + 4 + body.len());
    input.extend_from_slice(SIGNATURE_DOMAIN);
    input.extend_from_slice(&nonce.0);
    input.extend_from_slice(&length.to_be_bytes());
    input.extend_from_slice(body);
    input
}

fn decode_exact<const N: usize>(value: &str) -> Result<[u8; N], InviteError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| InviteError::Invalid)?;
    let bytes: [u8; N] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| InviteError::Invalid)?;
    (URL_SAFE_NO_PAD.encode(bytes) == value)
        .then_some(bytes)
        .ok_or(InviteError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_the_node_nonce_and_exact_body() {
        let key = NodeSigningKey::from_seed(&[0x21; 32]);
        let public = key.public_key();
        let nonce = EnrollmentNonce::from_bytes([0x31; 32]);
        let body = br#"{"session_token":"x"}"#;
        let header = EnrollmentProof::sign(&key, &nonce, body);
        EnrollmentProof::verify(&public, &nonce, body, Some(&header)).unwrap();

        let mismatch = Err(InviteError::NodeIdentityMismatch);
        let other_nonce = EnrollmentNonce::from_bytes([0x32; 32]);
        assert_eq!(
            EnrollmentProof::verify(&public, &other_nonce, body, Some(&header)),
            mismatch
        );
        assert_eq!(
            EnrollmentProof::verify(&public, &nonce, br#"{"session_token":"y"}"#, Some(&header)),
            mismatch
        );
        let other_key = NodeSigningKey::from_seed(&[0x22; 32]).public_key();
        assert_eq!(
            EnrollmentProof::verify(&other_key, &nonce, body, Some(&header)),
            mismatch
        );
        assert_eq!(
            EnrollmentProof::verify(&public, &nonce, body, None),
            mismatch
        );
        for malformed in [format!("{header}="), header[1..].to_owned(), String::new()] {
            assert_eq!(
                EnrollmentProof::verify(&public, &nonce, body, Some(&malformed)),
                mismatch
            );
        }
    }

    #[test]
    fn nonce_headers_are_exactly_32_canonical_base64url_bytes() {
        let nonce = EnrollmentNonce::generate();
        assert_eq!(
            EnrollmentNonce::from_header_value(&nonce.header_value()).unwrap(),
            nonce
        );
        assert_ne!(EnrollmentNonce::generate(), nonce);
        let valid = nonce.header_value();
        for invalid in [
            format!("{valid}="),
            valid[..42].to_owned(),
            format!("{valid}AA"),
            valid.replace(|character: char| character.is_ascii_alphabetic(), "+"),
        ] {
            assert!(
                EnrollmentNonce::from_header_value(&invalid).is_err(),
                "{invalid}"
            );
        }
        assert_eq!(format!("{nonce:?}"), "EnrollmentNonce(..)");
    }
}
