//! The authenticated content of an invite and its canonical payload.
//!
//! Payload: `[version, application_id, capability_type, node_public_key,
//! [locator...], capability, protocol_min, protocol_max, flags, expires_at]`
//! in canonical CBOR, where `flags` is always 0 and `expires_at` is a Unix
//! time or `null`.

use std::fmt;

use zeroize::Zeroizing;

use crate::{
    Capability, CapsuleError, InviteProfile, Locator, MAX_LOCATORS, NodePublicKey,
    cbor::{self, Decoder},
    locator::validate_locator_set,
    signature_input,
};

pub const INVITE_FORMAT_VERSION: u64 = 1;
pub const MAX_APPLICATION_ID_BYTES: usize = 64;

const PAYLOAD_FIELDS: usize = 10;
const FLAGS: u64 = 0;

/// A validated invite for profile `P`. Construct one with [`Self::new`] or by
/// decoding a [`crate::SignedInvite`]; either way every field is valid.
pub struct InviteCapsule<P: InviteProfile> {
    node_public_key: NodePublicKey,
    locators: Vec<Locator>,
    capability: P::Capability,
    protocol_min: u16,
    protocol_max: u16,
    expires_at_epoch_seconds: Option<u64>,
}

impl<P: InviteProfile> InviteCapsule<P> {
    pub fn new(
        node_public_key: NodePublicKey,
        locators: Vec<Locator>,
        capability: P::Capability,
        protocol_min: u16,
        protocol_max: u16,
        expires_at_epoch_seconds: Option<u64>,
    ) -> Result<Self, CapsuleError> {
        let capsule = Self {
            node_public_key,
            locators,
            capability,
            protocol_min,
            protocol_max,
            expires_at_epoch_seconds,
        };
        capsule.validate(None)?;
        Ok(capsule)
    }

    pub fn node_public_key(&self) -> &NodePublicKey {
        &self.node_public_key
    }

    pub fn locators(&self) -> &[Locator] {
        &self.locators
    }

    /// The bearer capability. Never log it.
    pub fn capability(&self) -> &P::Capability {
        &self.capability
    }

    pub fn protocol_min(&self) -> u16 {
        self.protocol_min
    }

    pub fn protocol_max(&self) -> u16 {
        self.protocol_max
    }

    pub fn supports_protocol(&self, version: u16) -> bool {
        (self.protocol_min..=self.protocol_max).contains(&version)
    }

    pub fn expires_at_epoch_seconds(&self) -> Option<u64> {
        self.expires_at_epoch_seconds
    }

    /// Whether the invite has expired at `now` (expiry is exclusive).
    pub fn is_expired_at(&self, now_epoch_seconds: u64) -> bool {
        self.expires_at_epoch_seconds
            .is_some_and(|expiry| expiry <= now_epoch_seconds)
    }

    /// Structure first, then (when `now` is given) expiry.
    pub(crate) fn validate(&self, now_epoch_seconds: Option<u64>) -> Result<(), CapsuleError> {
        if self.protocol_min == 0 || self.protocol_max < self.protocol_min {
            return Err(CapsuleError::Invalid);
        }
        validate_locator_set(&self.locators, P::LOCATORS)?;
        if now_epoch_seconds.is_some_and(|now| self.is_expired_at(now)) {
            return Err(CapsuleError::Expired);
        }
        Ok(())
    }

    pub(crate) fn canonical_payload(&self) -> Result<Zeroizing<Vec<u8>>, CapsuleError> {
        self.validate(None)?;
        let mut output = Zeroizing::new(Vec::with_capacity(256));
        cbor::encode_array(&mut output, PAYLOAD_FIELDS);
        cbor::encode_uint(&mut output, INVITE_FORMAT_VERSION);
        cbor::encode_text(&mut output, P::APPLICATION_ID);
        cbor::encode_uint(&mut output, P::CAPABILITY_TYPE);
        cbor::encode_bytes(&mut output, self.node_public_key.as_bytes());
        cbor::encode_array(&mut output, self.locators.len());
        for locator in &self.locators {
            locator.encode(&mut output);
        }
        self.capability.encode(&mut output);
        cbor::encode_uint(&mut output, self.protocol_min.into());
        cbor::encode_uint(&mut output, self.protocol_max.into());
        cbor::encode_uint(&mut output, FLAGS);
        cbor::encode_optional_uint(&mut output, self.expires_at_epoch_seconds);
        Ok(output)
    }

    /// Decodes a payload field by field, in order, then requires it to be the
    /// canonical encoding of what was read.
    pub(crate) fn decode_payload(payload: &[u8]) -> Result<Self, CapsuleError> {
        let mut decoder = Decoder::new(payload);
        decoder.array(PAYLOAD_FIELDS)?;
        if decoder.uint()? != INVITE_FORMAT_VERSION {
            return Err(CapsuleError::UnsupportedVersion);
        }
        if decoder.text(MAX_APPLICATION_ID_BYTES)? != P::APPLICATION_ID {
            return Err(CapsuleError::WrongApplication);
        }
        if decoder.uint()? != P::CAPABILITY_TYPE {
            return Err(CapsuleError::UnsupportedCapability);
        }
        let node_key = decoder
            .bytes(32)?
            .try_into()
            .map_err(|_| CapsuleError::Invalid)?;
        let node_public_key = NodePublicKey::from_bytes(node_key)?;
        let locator_count = decoder.array_len()?;
        if locator_count == 0 || locator_count > MAX_LOCATORS {
            return Err(CapsuleError::Invalid);
        }
        let locators = (0..locator_count)
            .map(|_| Locator::decode(&mut decoder, P::LOCATORS))
            .collect::<Result<Vec<_>, _>>()?;
        let capability = P::Capability::decode(&mut decoder)?;
        let protocol_min = protocol_version(&mut decoder)?;
        let protocol_max = protocol_version(&mut decoder)?;
        if decoder.uint()? != FLAGS {
            return Err(CapsuleError::Invalid);
        }
        let expires_at_epoch_seconds = decoder.optional_uint()?;
        decoder.finish()?;
        let capsule = Self {
            node_public_key,
            locators,
            capability,
            protocol_min,
            protocol_max,
            expires_at_epoch_seconds,
        };
        if capsule.canonical_payload()?.as_slice() != payload {
            return Err(CapsuleError::Invalid);
        }
        Ok(capsule)
    }

    pub(crate) fn signature_input(&self) -> Result<Zeroizing<Vec<u8>>, CapsuleError> {
        let payload = self.canonical_payload()?;
        Ok(Zeroizing::new(signature_input(
            P::SIGNATURE_DOMAIN,
            P::APPLICATION_ID,
            &payload,
        )))
    }
}

impl<P: InviteProfile> Clone for InviteCapsule<P> {
    fn clone(&self) -> Self {
        Self {
            node_public_key: self.node_public_key,
            locators: self.locators.clone(),
            capability: self.capability.clone(),
            protocol_min: self.protocol_min,
            protocol_max: self.protocol_max,
            expires_at_epoch_seconds: self.expires_at_epoch_seconds,
        }
    }
}

impl<P: InviteProfile> PartialEq for InviteCapsule<P> {
    fn eq(&self, other: &Self) -> bool {
        self.node_public_key == other.node_public_key
            && self.locators == other.locators
            && self.capability == other.capability
            && self.protocol_min == other.protocol_min
            && self.protocol_max == other.protocol_max
            && self.expires_at_epoch_seconds == other.expires_at_epoch_seconds
    }
}

impl<P: InviteProfile> Eq for InviteCapsule<P> {}

impl<P: InviteProfile> fmt::Debug for InviteCapsule<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InviteCapsule")
            .field("application_id", &P::APPLICATION_ID)
            .field("node_public_key", &self.node_public_key)
            .field("locators", &self.locators)
            .field("capability", &self.capability)
            .field("protocol_min", &self.protocol_min)
            .field("protocol_max", &self.protocol_max)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .finish()
    }
}

fn protocol_version(decoder: &mut Decoder<'_>) -> Result<u16, CapsuleError> {
    u16::try_from(decoder.uint()?).map_err(|_| CapsuleError::Invalid)
}
