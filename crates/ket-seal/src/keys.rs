//! The node's sealing keys, derived from its Ed25519 identity.

use ed25519_dalek::{SigningKey, VerifyingKey};
use hpke::{Deserializable, kem::Kem as _};
use zeroize::Zeroizing;

use crate::{Kem, SealError};

/// What an app seals to: the X25519 form of the node's pinned Ed25519 key.
#[derive(Clone)]
pub struct NodeSealingKey {
    pub(crate) ed25519: [u8; 32],
    pub(crate) x25519: <Kem as hpke::Kem>::PublicKey,
}

impl NodeSealingKey {
    /// `ed25519` is the node key from an invite. Small-order and other
    /// invalid points are refused.
    pub fn from_ed25519(ed25519: &[u8; 32]) -> Result<Self, SealError> {
        let verifying = VerifyingKey::from_bytes(ed25519).map_err(|_| SealError)?;
        if verifying.is_weak() {
            return Err(SealError);
        }
        let x25519 =
            <Kem as hpke::Kem>::PublicKey::from_bytes(verifying.to_montgomery().as_bytes())
                .map_err(|_| SealError)?;
        Ok(Self {
            ed25519: *ed25519,
            x25519,
        })
    }
}

impl std::fmt::Debug for NodeSealingKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NodeSealingKey(..)")
    }
}

/// The node's private half, held only by the node.
pub struct NodeOpeningKey {
    pub(crate) ed25519: [u8; 32],
    pub(crate) x25519: <Kem as hpke::Kem>::PrivateKey,
}

impl NodeOpeningKey {
    /// `seed` is the node's Ed25519 signing seed. The X25519 scalar is the
    /// Ed25519 secret scalar, so it matches [`NodeSealingKey::from_ed25519`].
    pub fn from_ed25519_seed(seed: &[u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(seed);
        let scalar = Zeroizing::new(signing.to_scalar_bytes());
        let x25519 = <Kem as hpke::Kem>::PrivateKey::from_bytes(scalar.as_slice())
            .expect("any 32 bytes are an X25519 private key");
        Self {
            ed25519: signing.verifying_key().to_bytes(),
            x25519,
        }
    }

    pub fn sealing_key(&self) -> NodeSealingKey {
        NodeSealingKey {
            ed25519: self.ed25519,
            x25519: Kem::sk_to_pk(&self.x25519),
        }
    }
}

impl std::fmt::Debug for NodeOpeningKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NodeOpeningKey(..)")
    }
}
