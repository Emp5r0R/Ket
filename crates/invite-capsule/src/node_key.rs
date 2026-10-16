//! Node identity keys. A node holds the signing seed; clients pin only the
//! public key carried by an invite.

use data_encoding::BASE64URL_NOPAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::CapsuleError;

/// A node's persistent Ed25519 signing key. The seed is zeroized on drop and
/// never printed.
pub struct NodeSigningKey(SigningKey);

impl NodeSigningKey {
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(seed))
    }

    /// Parses a 32-byte seed written as unpadded Base64URL.
    pub fn from_base64url(value: &str) -> Result<Self, CapsuleError> {
        let seed = Zeroizing::new(decode_exact::<32>(value).ok_or(CapsuleError::InvalidNodeKey)?);
        Ok(Self::from_seed(&seed))
    }

    /// The seed as unpadded Base64URL, for the node's own configuration.
    pub fn to_base64url(&self) -> Zeroizing<String> {
        Zeroizing::new(BASE64URL_NOPAD.encode(Zeroizing::new(self.0.to_bytes()).as_slice()))
    }

    pub fn public_key(&self) -> NodePublicKey {
        NodePublicKey(self.0.verifying_key().to_bytes())
    }

    /// Signs `message`. Callers build `message` with a domain-separated
    /// input such as [`crate::signature_input`].
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

impl std::fmt::Debug for NodeSigningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("NodeSigningKey")
            .field(&self.public_key())
            .finish()
    }
}

/// A node's Ed25519 public key: nonzero and a valid curve point.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct NodePublicKey([u8; 32]);

impl NodePublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self, CapsuleError> {
        if bytes == [0_u8; 32] {
            return Err(CapsuleError::InvalidNodeKey);
        }
        VerifyingKey::from_bytes(&bytes).map_err(|_| CapsuleError::InvalidNodeKey)?;
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Unpadded Base64URL, the form clients persist with a saved node.
    pub fn to_base64url(&self) -> String {
        BASE64URL_NOPAD.encode(&self.0)
    }

    pub fn from_base64url(value: &str) -> Result<Self, CapsuleError> {
        Self::from_bytes(decode_exact::<32>(value).ok_or(CapsuleError::InvalidNodeKey)?)
    }

    /// A short, human-comparable identity such as `1A2B:3C4D:...`: the first
    /// 16 bytes of `SHA-256(domain || key)`. Each application picks its own
    /// domain so the same key reads differently in different applications.
    pub fn fingerprint(&self, domain: &[u8]) -> String {
        key_fingerprint(domain, &self.0)
    }

    /// Strict Ed25519 verification (no small-order keys or malleable
    /// signatures).
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
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
            .field(&self.to_base64url())
            .finish()
    }
}

/// [`NodePublicKey::fingerprint`] for raw key bytes.
pub fn key_fingerprint(domain: &[u8], key: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(key);
    digest.finalize()[..16]
        .chunks(2)
        .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
        .collect::<Vec<_>>()
        .join(":")
}

/// Decodes exactly `N` bytes of canonical unpadded Base64URL.
pub(crate) fn decode_exact<const N: usize>(value: &str) -> Option<[u8; N]> {
    let decoded = Zeroizing::new(BASE64URL_NOPAD.decode(value.as_bytes()).ok()?);
    if BASE64URL_NOPAD.encode(&decoded) != value {
        return None;
    }
    decoded.as_slice().try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_text_round_trips_and_rejects_noncanonical_forms() {
        let seed = [7_u8; 32];
        let text = BASE64URL_NOPAD.encode(&seed);
        let key = NodeSigningKey::from_base64url(&text).unwrap();
        assert_eq!(
            key.public_key(),
            NodeSigningKey::from_seed(&seed).public_key()
        );
        assert_eq!(key.to_base64url().as_str(), text);

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
    fn public_keys_reject_zero_and_invalid_points() {
        assert_eq!(
            NodePublicKey::from_bytes([0; 32]),
            Err(CapsuleError::InvalidNodeKey)
        );
        // About half of all y values have no x on the curve.
        let not_a_point = (2_u8..=255)
            .map(|y| {
                let mut bytes = [0_u8; 32];
                bytes[0] = y;
                bytes
            })
            .find(|bytes| VerifyingKey::from_bytes(bytes).is_err())
            .expect("a y coordinate off the curve");
        assert_eq!(
            NodePublicKey::from_bytes(not_a_point),
            Err(CapsuleError::InvalidNodeKey)
        );
    }

    #[test]
    fn debug_output_shows_only_the_public_key() {
        let key = NodeSigningKey::from_seed(&[9; 32]);
        let debug = format!("{key:?}");
        assert!(debug.contains(&key.public_key().to_base64url()));
        assert!(!debug.contains(&BASE64URL_NOPAD.encode(&[9_u8; 32])));
    }

    #[test]
    fn fingerprints_are_domain_separated() {
        let key = NodeSigningKey::from_seed(&[9; 32]).public_key();
        let first = key.fingerprint(b"APP-ONE");
        assert_eq!(first.len(), 39);
        assert_ne!(first, key.fingerprint(b"APP-TWO"));
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
            BASE64URL_NOPAD.encode(&[0_u8; 32]),
            "not base64!".to_owned(),
        ] {
            assert_eq!(
                NodePublicKey::from_base64url(&invalid),
                Err(CapsuleError::InvalidNodeKey),
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
