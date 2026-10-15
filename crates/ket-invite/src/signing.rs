//! Node identity keys. The server holds the signing seed; clients pin only the
//! public key carried by an invite.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::InviteError;

const FINGERPRINT_DOMAIN: &[u8] = b"KET-NODE-FINGERPRINT-V1";

/// The node's persistent Ed25519 signing key. The seed is zeroized on drop and
/// never printed.
pub struct NodeSigningKey(SigningKey);

impl NodeSigningKey {
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(seed))
    }

    /// Parses the `.env` form: a 32-byte seed as Base64URL without padding.
    pub fn from_base64url(value: &str) -> Result<Self, InviteError> {
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| InviteError::InvalidNodeKey)?,
        );
        let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
            decoded
                .as_slice()
                .try_into()
                .map_err(|_| InviteError::InvalidNodeKey)?,
        );
        if URL_SAFE_NO_PAD.encode(seed.as_slice()) != value {
            return Err(InviteError::InvalidNodeKey);
        }
        Ok(Self::from_seed(&seed))
    }

    pub fn public_key(&self) -> NodePublicKey {
        NodePublicKey(self.0.verifying_key().to_bytes())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

impl std::fmt::Debug for NodeSigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("NodeSigningKey")
            .field(&self.public_key().fingerprint())
            .finish()
    }
}

/// A node's Ed25519 public key as pinned from an invite.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct NodePublicKey([u8; 32]);

impl NodePublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self, InviteError> {
        if bytes == [0_u8; 32] {
            return Err(InviteError::InvalidNodeKey);
        }
        VerifyingKey::from_bytes(&bytes).map_err(|_| InviteError::InvalidNodeKey)?;
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Unpadded Base64URL, the form clients persist with a saved profile.
    pub fn to_base64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64url(value: &str) -> Result<Self, InviteError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| InviteError::InvalidNodeKey)?;
        let bytes: [u8; 32] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| InviteError::InvalidNodeKey)?;
        if URL_SAFE_NO_PAD.encode(bytes) != value {
            return Err(InviteError::InvalidNodeKey);
        }
        Self::from_bytes(bytes)
    }

    /// A short, human-comparable identity such as `1A2B:3C4D:...` (16 bytes).
    pub fn fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(FINGERPRINT_DOMAIN);
        digest.update(self.0);
        digest.finalize()[..16]
            .chunks(2)
            .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join(":")
    }

    pub(crate) fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        VerifyingKey::from_bytes(&self.0).is_ok_and(|key| {
            key.verify_strict(message, &Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

impl std::fmt::Debug for NodePublicKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("NodePublicKey")
            .field(&self.fingerprint())
            .finish()
    }
}

/// `domain || u16be(len(application_id)) || application_id || payload`.
pub(crate) fn signature_input(domain: &[u8], application_id: &str, payload: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(domain.len() + 2 + application_id.len() + payload.len());
    input.extend_from_slice(domain);
    input.extend_from_slice(&(application_id.len() as u16).to_be_bytes());
    input.extend_from_slice(application_id.as_bytes());
    input.extend_from_slice(payload);
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_text_round_trips_and_rejects_noncanonical_forms() {
        let seed = [7_u8; 32];
        let text = URL_SAFE_NO_PAD.encode(seed);
        let key = NodeSigningKey::from_base64url(&text).unwrap();
        assert_eq!(
            key.public_key(),
            NodeSigningKey::from_seed(&seed).public_key()
        );

        for invalid in [
            format!("{text}="),
            text[..42].to_owned(),
            format!("{text}AA"),
            text.replace('B', "+"),
            String::new(),
        ] {
            assert!(
                NodeSigningKey::from_base64url(&invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn public_keys_reject_zero_and_debug_output_hides_the_seed() {
        assert_eq!(
            NodePublicKey::from_bytes([0; 32]),
            Err(InviteError::InvalidNodeKey)
        );
        let key = NodeSigningKey::from_seed(&[9; 32]);
        let fingerprint = key.public_key().fingerprint();
        assert_eq!(fingerprint.len(), 39);
        let debug = format!("{key:?}");
        assert!(debug.contains(&fingerprint));
        assert!(!debug.contains(&URL_SAFE_NO_PAD.encode([9_u8; 32])));
    }

    #[test]
    fn public_key_text_round_trips_and_rejects_noncanonical_forms() {
        let key = NodeSigningKey::from_seed(&[5; 32]).public_key();
        let text = key.to_base64url();
        assert_eq!(text.len(), 43);
        assert_eq!(NodePublicKey::from_base64url(&text), Ok(key));
        for invalid in [
            format!("{text}="),
            text[1..].to_owned(),
            URL_SAFE_NO_PAD.encode([0_u8; 32]),
            "not base64!".to_owned(),
        ] {
            assert_eq!(
                NodePublicKey::from_base64url(&invalid),
                Err(InviteError::InvalidNodeKey),
                "{invalid}"
            );
        }
    }

    #[test]
    fn signatures_verify_only_for_the_signed_message_and_key() {
        let key = NodeSigningKey::from_seed(&[3; 32]);
        let signature = key.sign(b"message");
        assert!(key.public_key().verify(b"message", &signature));
        assert!(!key.public_key().verify(b"messagf", &signature));
        let other = NodeSigningKey::from_seed(&[4; 32]).public_key();
        assert!(!other.verify(b"message", &signature));
    }
}
