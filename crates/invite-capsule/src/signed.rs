//! The node-signed invite and its canonical binary and text forms.
//!
//! Binary: `[payload_bytes, signature]` in canonical CBOR, where the Ed25519
//! signature covers [`crate::signature_input`] of the profile's domain,
//! application ID, and payload.

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

use crate::{
    CapsuleError, InviteCapsule, InviteProfile, NodeSigningKey,
    cbor::{self, Decoder},
    text,
};

pub const MAX_BINARY_INVITE_BYTES: usize = 1_024;
pub const MAX_ENCODED_INVITE_TEXT_BYTES: usize = 2_048;

const SIGNED_FIELDS: usize = 2;
const SIGNATURE_BYTES: usize = 64;

/// An invite capsule with its node's signature. Decoded values are always
/// fully verified.
pub struct SignedInvite<P: InviteProfile> {
    capsule: InviteCapsule<P>,
    signature: [u8; SIGNATURE_BYTES],
}

impl<P: InviteProfile> SignedInvite<P> {
    pub fn sign(
        capsule: InviteCapsule<P>,
        signing_key: &NodeSigningKey,
    ) -> Result<Self, CapsuleError> {
        if signing_key.public_key() != *capsule.node_public_key() {
            return Err(CapsuleError::NodeIdentityMismatch);
        }
        let signature = signing_key.sign(&capsule.signature_input()?);
        Ok(Self { capsule, signature })
    }

    /// Reassembles a signed invite from its parts and verifies it, so an
    /// unverified value never exists.
    pub fn from_parts(
        capsule: InviteCapsule<P>,
        signature: [u8; SIGNATURE_BYTES],
        now_epoch_seconds: Option<u64>,
    ) -> Result<Self, CapsuleError> {
        let signed = Self { capsule, signature };
        signed.verify(now_epoch_seconds)?;
        Ok(signed)
    }

    pub fn capsule(&self) -> &InviteCapsule<P> {
        &self.capsule
    }

    pub fn signature(&self) -> &[u8; SIGNATURE_BYTES] {
        &self.signature
    }

    /// Checks structure, then the signature, then (when `now` is given)
    /// expiry, so an expired forgery reports a bad signature.
    pub fn verify(&self, now_epoch_seconds: Option<u64>) -> Result<(), CapsuleError> {
        if !self
            .capsule
            .node_public_key()
            .verify(&self.capsule.signature_input()?, &self.signature)
        {
            return Err(CapsuleError::InvalidSignature);
        }
        self.capsule.validate(now_epoch_seconds)
    }

    pub fn canonical_binary(&self) -> Result<Zeroizing<Vec<u8>>, CapsuleError> {
        let payload = self.capsule.canonical_payload()?;
        let mut output = Zeroizing::new(Vec::with_capacity(payload.len() + 70));
        cbor::encode_array(&mut output, SIGNED_FIELDS);
        cbor::encode_bytes(&mut output, &payload);
        cbor::encode_bytes(&mut output, &self.signature);
        if output.len() > MAX_BINARY_INVITE_BYTES {
            return Err(CapsuleError::TooLarge);
        }
        Ok(output)
    }

    /// Decodes and fully verifies a canonical binary invite.
    pub fn decode(binary: &[u8], now_epoch_seconds: Option<u64>) -> Result<Self, CapsuleError> {
        if binary.is_empty() || binary.len() > MAX_BINARY_INVITE_BYTES {
            return Err(CapsuleError::TooLarge);
        }
        let mut decoder = Decoder::new(binary);
        decoder.array(SIGNED_FIELDS)?;
        let payload = decoder.bytes(MAX_BINARY_INVITE_BYTES)?;
        let signature = decoder
            .bytes(SIGNATURE_BYTES)?
            .try_into()
            .map_err(|_| CapsuleError::Invalid)?;
        decoder.finish()?;
        let signed = Self {
            capsule: InviteCapsule::decode_payload(payload)?,
            signature,
        };
        signed.verify(now_epoch_seconds)?;
        if signed.canonical_binary()?.as_slice() != binary {
            return Err(CapsuleError::Invalid);
        }
        Ok(signed)
    }

    /// `<prefix><base64url>`, the form for links, QR codes, and pasting.
    pub fn to_text(&self) -> Result<Zeroizing<String>, CapsuleError> {
        Ok(text::encode_base64url(
            P::TEXT_PREFIX,
            &self.canonical_binary()?,
        ))
    }

    /// The profile's hand-typeable form; [`CapsuleError::Invalid`] if the
    /// profile has none.
    pub fn to_manual_text(&self) -> Result<Zeroizing<String>, CapsuleError> {
        let form = P::MANUAL_TEXT.ok_or(CapsuleError::Invalid)?;
        Ok(text::encode_manual(form, &self.canonical_binary()?))
    }

    /// Decodes and fully verifies either text form. Surrounding whitespace,
    /// padding, or any non-canonical encoding is rejected.
    pub fn from_text(input: &str, now_epoch_seconds: Option<u64>) -> Result<Self, CapsuleError> {
        if input.is_empty() || input.len() > MAX_ENCODED_INVITE_TEXT_BYTES {
            return Err(CapsuleError::TooLarge);
        }
        let binary = match P::MANUAL_TEXT {
            Some(form) if text::has_prefix(input, form.prefix) => text::decode_manual(form, input)?,
            _ => text::decode_base64url(P::TEXT_PREFIX, input)?,
        };
        Self::decode(&binary, now_epoch_seconds)
    }
}

impl<P: InviteProfile> Clone for SignedInvite<P> {
    fn clone(&self) -> Self {
        Self {
            capsule: self.capsule.clone(),
            signature: self.signature,
        }
    }
}

impl<P: InviteProfile> PartialEq for SignedInvite<P> {
    fn eq(&self, other: &Self) -> bool {
        self.capsule == other.capsule && self.signature == other.signature
    }
}

impl<P: InviteProfile> Eq for SignedInvite<P> {}

impl<P: InviteProfile> fmt::Debug for SignedInvite<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignedInvite")
            .field("capsule", &self.capsule)
            .finish_non_exhaustive()
    }
}

impl<P: InviteProfile> Drop for SignedInvite<P> {
    fn drop(&mut self) {
        self.signature.zeroize();
    }
}
