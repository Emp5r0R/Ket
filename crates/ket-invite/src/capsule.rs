//! The signed invite object and its canonical binary form.

use zeroize::{Zeroize, Zeroizing};

use crate::{
    ControlLocator, InviteError, NodePublicKey, NodeSigningKey,
    codec::{self, Decoder},
    locator::{MAX_LOCATORS, validate_locator_set},
    signing::signature_input,
};

pub const KET_APPLICATION_ID: &str = "org.ket.access";
pub const INVITE_FORMAT_VERSION: u64 = 1;
pub const CONTROL_API_VERSION: u16 = 1;
pub const ACCESS_CODE_BYTES: usize = 32;
pub const MAX_BINARY_INVITE_BYTES: usize = 1_024;
pub const MAX_ENCODED_INVITE_TEXT_BYTES: usize = 2_048;

const SIGNATURE_DOMAIN: &[u8] = b"KET-INVITE-CAPSULE-V1";
const CAPABILITY_ACCESS_GRANT: u64 = 1;
const PAYLOAD_FIELDS: usize = 10;
const SIGNED_FIELDS: usize = 2;
const MAX_APPLICATION_ID_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 64;

/// The authenticated content of an invite.
#[derive(Clone, Eq, PartialEq)]
pub struct InviteCapsuleV1 {
    pub node_public_key: NodePublicKey,
    pub locators: Vec<ControlLocator>,
    access_code: Zeroizing<String>,
    pub control_api_min: u16,
    pub control_api_max: u16,
    pub access_expires_at_epoch_seconds: Option<u64>,
}

impl InviteCapsuleV1 {
    pub fn new(
        node_public_key: NodePublicKey,
        locators: Vec<ControlLocator>,
        access_code: &str,
        access_expires_at_epoch_seconds: Option<u64>,
    ) -> Result<Self, InviteError> {
        let capsule = Self {
            node_public_key,
            locators,
            access_code: Zeroizing::new(access_code.to_owned()),
            control_api_min: CONTROL_API_VERSION,
            control_api_max: CONTROL_API_VERSION,
            access_expires_at_epoch_seconds,
        };
        capsule.validate(None)?;
        Ok(capsule)
    }

    /// The bearer access code. Never log it.
    pub fn access_code(&self) -> &str {
        &self.access_code
    }

    pub fn supports_control_api(&self, version: u16) -> bool {
        (self.control_api_min..=self.control_api_max).contains(&version)
    }

    fn validate(&self, now_epoch_seconds: Option<u64>) -> Result<(), InviteError> {
        if self.access_code.len() != ACCESS_CODE_BYTES
            || !self
                .access_code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || self.control_api_min == 0
            || self.control_api_max < self.control_api_min
        {
            return Err(InviteError::Invalid);
        }
        validate_locator_set(&self.locators)?;
        if now_epoch_seconds.is_some_and(|now| {
            self.access_expires_at_epoch_seconds
                .is_some_and(|expiry| expiry <= now)
        }) {
            return Err(InviteError::Expired);
        }
        Ok(())
    }

    fn canonical_payload(&self) -> Result<Zeroizing<Vec<u8>>, InviteError> {
        self.validate(None)?;
        let mut output = Zeroizing::new(Vec::with_capacity(256));
        codec::encode_array(&mut output, PAYLOAD_FIELDS);
        codec::encode_uint(&mut output, INVITE_FORMAT_VERSION);
        codec::encode_text(&mut output, KET_APPLICATION_ID);
        codec::encode_uint(&mut output, CAPABILITY_ACCESS_GRANT);
        codec::encode_bytes(&mut output, self.node_public_key.as_bytes());
        codec::encode_array(&mut output, self.locators.len());
        for locator in &self.locators {
            locator.encode(&mut output);
        }
        codec::encode_text(&mut output, &self.access_code);
        codec::encode_uint(&mut output, self.control_api_min.into());
        codec::encode_uint(&mut output, self.control_api_max.into());
        codec::encode_uint(&mut output, 0);
        codec::encode_optional_uint(&mut output, self.access_expires_at_epoch_seconds);
        Ok(output)
    }

    fn decode_payload(payload: &[u8]) -> Result<Self, InviteError> {
        let mut decoder = Decoder::new(payload);
        decoder.array(PAYLOAD_FIELDS)?;
        if decoder.uint()? != INVITE_FORMAT_VERSION {
            return Err(InviteError::UnsupportedVersion);
        }
        if decoder.text(MAX_APPLICATION_ID_BYTES)? != KET_APPLICATION_ID {
            return Err(InviteError::WrongApplication);
        }
        if decoder.uint()? != CAPABILITY_ACCESS_GRANT {
            return Err(InviteError::UnsupportedCapability);
        }
        let node_public_key = NodePublicKey::from_bytes(exact::<32>(decoder.bytes(32)?)?)?;
        let locator_count = decoder.array_len()?;
        if locator_count == 0 || locator_count > MAX_LOCATORS {
            return Err(InviteError::Invalid);
        }
        let locators = (0..locator_count)
            .map(|_| ControlLocator::decode(&mut decoder))
            .collect::<Result<Vec<_>, _>>()?;
        let access_code = Zeroizing::new(decoder.text(ACCESS_CODE_BYTES)?.to_owned());
        let control_api_min = version(&mut decoder)?;
        let control_api_max = version(&mut decoder)?;
        if decoder.uint()? != 0 {
            return Err(InviteError::Invalid);
        }
        let access_expires_at_epoch_seconds = decoder.optional_uint()?;
        decoder.finish()?;
        let capsule = Self {
            node_public_key,
            locators,
            access_code,
            control_api_min,
            control_api_max,
            access_expires_at_epoch_seconds,
        };
        if capsule.canonical_payload()?.as_slice() != payload {
            return Err(InviteError::Invalid);
        }
        Ok(capsule)
    }

    fn signature_input(&self) -> Result<Zeroizing<Vec<u8>>, InviteError> {
        let payload = self.canonical_payload()?;
        Ok(Zeroizing::new(signature_input(
            SIGNATURE_DOMAIN,
            KET_APPLICATION_ID,
            &payload,
        )))
    }
}

impl std::fmt::Debug for InviteCapsuleV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InviteCapsuleV1")
            .field("node_public_key", &self.node_public_key)
            .field("locators", &self.locators)
            .field("access_code", &"<redacted>")
            .field("control_api_min", &self.control_api_min)
            .field("control_api_max", &self.control_api_max)
            .field(
                "access_expires_at_epoch_seconds",
                &self.access_expires_at_epoch_seconds,
            )
            .finish()
    }
}

/// An invite capsule with the node's signature over its canonical payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedInviteCapsule {
    capsule: InviteCapsuleV1,
    signature: [u8; SIGNATURE_BYTES],
}

impl SignedInviteCapsule {
    pub fn sign(
        capsule: InviteCapsuleV1,
        signing_key: &NodeSigningKey,
    ) -> Result<Self, InviteError> {
        if signing_key.public_key() != capsule.node_public_key {
            return Err(InviteError::NodeIdentityMismatch);
        }
        let signature = signing_key.sign(&capsule.signature_input()?);
        Ok(Self { capsule, signature })
    }

    pub fn capsule(&self) -> &InviteCapsuleV1 {
        &self.capsule
    }

    /// Checks structure, then the signature, then (when `now` is given)
    /// expiry, so an expired forgery reports a bad signature.
    pub fn verify(&self, now_epoch_seconds: Option<u64>) -> Result<(), InviteError> {
        if !self
            .capsule
            .node_public_key
            .verify(&self.capsule.signature_input()?, &self.signature)
        {
            return Err(InviteError::InvalidSignature);
        }
        self.capsule.validate(now_epoch_seconds)
    }

    pub fn canonical_binary(&self) -> Result<Zeroizing<Vec<u8>>, InviteError> {
        let payload = self.capsule.canonical_payload()?;
        let mut output = Zeroizing::new(Vec::with_capacity(payload.len() + 70));
        codec::encode_array(&mut output, SIGNED_FIELDS);
        codec::encode_bytes(&mut output, &payload);
        codec::encode_bytes(&mut output, &self.signature);
        if output.len() > MAX_BINARY_INVITE_BYTES {
            return Err(InviteError::TooLarge);
        }
        Ok(output)
    }

    /// Decodes and fully verifies a canonical binary invite.
    pub fn decode(binary: &[u8], now_epoch_seconds: Option<u64>) -> Result<Self, InviteError> {
        if binary.is_empty() || binary.len() > MAX_BINARY_INVITE_BYTES {
            return Err(InviteError::TooLarge);
        }
        let mut decoder = Decoder::new(binary);
        decoder.array(SIGNED_FIELDS)?;
        let payload = decoder.bytes(MAX_BINARY_INVITE_BYTES)?;
        let signature = exact::<SIGNATURE_BYTES>(decoder.bytes(SIGNATURE_BYTES)?)?;
        decoder.finish()?;
        let signed = Self {
            capsule: InviteCapsuleV1::decode_payload(payload)?,
            signature,
        };
        signed.verify(now_epoch_seconds)?;
        if signed.canonical_binary()?.as_slice() != binary {
            return Err(InviteError::Invalid);
        }
        Ok(signed)
    }
}

impl Drop for SignedInviteCapsule {
    fn drop(&mut self) {
        self.signature.zeroize();
    }
}

fn version(decoder: &mut Decoder<'_>) -> Result<u16, InviteError> {
    u16::try_from(decoder.uint()?).map_err(|_| InviteError::Invalid)
}

fn exact<const N: usize>(value: &[u8]) -> Result<[u8; N], InviteError> {
    value.try_into().map_err(|_| InviteError::Invalid)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const CODE: &str = "Abcdefghijklmnopqrstuvwxyz012345";

    fn key() -> NodeSigningKey {
        NodeSigningKey::from_seed(&[0x11; 32])
    }

    fn capsule(expiry: Option<u64>) -> InviteCapsuleV1 {
        InviteCapsuleV1::new(
            key().public_key(),
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            CODE,
            expiry,
        )
        .unwrap()
    }

    fn signed(expiry: Option<u64>) -> SignedInviteCapsule {
        SignedInviteCapsule::sign(capsule(expiry), &key()).unwrap()
    }

    #[test]
    fn signed_invites_round_trip_through_the_canonical_binary() {
        let invite = signed(Some(2_000_000_000));
        let binary = invite.canonical_binary().unwrap();
        let decoded = SignedInviteCapsule::decode(&binary, Some(1_900_000_000)).unwrap();
        assert_eq!(decoded, invite);
        assert_eq!(decoded.capsule().access_code(), CODE);
        assert!(decoded.capsule().supports_control_api(CONTROL_API_VERSION));
        assert!(!decoded.capsule().supports_control_api(2));
        let unbounded = signed(None);
        assert_eq!(
            SignedInviteCapsule::decode(&unbounded.canonical_binary().unwrap(), Some(u64::MAX))
                .unwrap()
                .capsule()
                .access_expires_at_epoch_seconds,
            None
        );
    }

    #[test]
    fn every_bit_of_the_binary_is_authenticated_or_structural() {
        let binary = signed(Some(2_000_000_000)).canonical_binary().unwrap();
        for index in 0..binary.len() {
            for bit in 0..8 {
                let mut tampered = binary.to_vec();
                tampered[index] ^= 1 << bit;
                assert!(
                    SignedInviteCapsule::decode(&tampered, None).is_err(),
                    "byte {index} bit {bit}"
                );
            }
        }
    }

    #[test]
    fn signing_requires_the_node_key_named_in_the_capsule() {
        let other = NodeSigningKey::from_seed(&[0x12; 32]);
        assert_eq!(
            SignedInviteCapsule::sign(capsule(None), &other),
            Err(InviteError::NodeIdentityMismatch)
        );
    }

    #[test]
    fn access_codes_must_be_exactly_32_ascii_alphanumerics() {
        for code in [
            "short",
            "Abcdefghijklmnopqrstuvwxyz0123456",
            "Abcdefghijklmnopqrstuvwxyz01234-",
            "Abcdefghijklmnopqrstuvwxyz01234é",
        ] {
            assert!(
                InviteCapsuleV1::new(
                    key().public_key(),
                    vec![ControlLocator::https("ket.example.com", 443).unwrap()],
                    code,
                    None
                )
                .is_err(),
                "{code}"
            );
        }
    }

    #[test]
    fn expiry_is_enforced_only_when_a_clock_is_supplied() {
        let invite = signed(Some(2_000_000_000));
        assert_eq!(
            invite.verify(Some(2_000_000_000)),
            Err(InviteError::Expired)
        );
        assert!(invite.verify(Some(1_999_999_999)).is_ok());
        assert!(invite.verify(None).is_ok());
    }

    /// Signs arbitrary payload bytes so tests can reach semantic checks that
    /// sit behind a valid signature.
    pub(crate) fn sign_raw_payload(payload: &[u8], signing_key: &NodeSigningKey) -> Vec<u8> {
        let signature = signing_key.sign(&signature_input(
            SIGNATURE_DOMAIN,
            KET_APPLICATION_ID,
            payload,
        ));
        let mut output = Vec::new();
        codec::encode_array(&mut output, SIGNED_FIELDS);
        codec::encode_bytes(&mut output, payload);
        codec::encode_bytes(&mut output, &signature);
        output
    }

    /// Field-by-field payload builder; each override replaces one field's
    /// encoded bytes.
    pub(crate) struct RawPayload {
        pub fields: Vec<Vec<u8>>,
    }

    impl RawPayload {
        pub(crate) fn valid(public_key: &NodePublicKey) -> Self {
            let mut fields = vec![Vec::new(); PAYLOAD_FIELDS];
            codec::encode_uint(&mut fields[0], INVITE_FORMAT_VERSION);
            codec::encode_text(&mut fields[1], KET_APPLICATION_ID);
            codec::encode_uint(&mut fields[2], CAPABILITY_ACCESS_GRANT);
            codec::encode_bytes(&mut fields[3], public_key.as_bytes());
            codec::encode_array(&mut fields[4], 1);
            ControlLocator::https("ket.example.com", 443)
                .unwrap()
                .encode(&mut fields[4]);
            codec::encode_text(&mut fields[5], CODE);
            codec::encode_uint(&mut fields[6], 1);
            codec::encode_uint(&mut fields[7], 1);
            codec::encode_uint(&mut fields[8], 0);
            codec::encode_uint(&mut fields[9], 2_000_000_000);
            Self { fields }
        }

        pub(crate) fn with(mut self, index: usize, encode: impl FnOnce(&mut Vec<u8>)) -> Self {
            self.fields[index].clear();
            encode(&mut self.fields[index]);
            self
        }

        pub(crate) fn bytes(&self) -> Vec<u8> {
            let mut output = Vec::new();
            codec::encode_array(&mut output, PAYLOAD_FIELDS);
            for field in &self.fields {
                output.extend_from_slice(field);
            }
            output
        }
    }

    #[test]
    fn signed_but_semantically_invalid_payloads_are_rejected() {
        let key = key();
        let public = key.public_key();
        let decode = |payload: RawPayload| {
            SignedInviteCapsule::decode(&sign_raw_payload(&payload.bytes(), &key), None)
        };
        assert!(decode(RawPayload::valid(&public)).is_ok());
        let cases: Vec<(RawPayload, InviteError)> = vec![
            (
                RawPayload::valid(&public).with(0, |out| codec::encode_uint(out, 2)),
                InviteError::UnsupportedVersion,
            ),
            (
                RawPayload::valid(&public).with(1, |out| codec::encode_text(out, "org.other.app")),
                InviteError::WrongApplication,
            ),
            (
                RawPayload::valid(&public).with(2, |out| codec::encode_uint(out, 2)),
                InviteError::UnsupportedCapability,
            ),
            (
                RawPayload::valid(&public).with(3, |out| codec::encode_bytes(out, &[0; 32])),
                InviteError::InvalidNodeKey,
            ),
            (
                RawPayload::valid(&public).with(4, |out| codec::encode_array(out, 0)),
                InviteError::Invalid,
            ),
            (
                RawPayload::valid(&public).with(5, |out| codec::encode_text(out, "short")),
                InviteError::Invalid,
            ),
            (
                RawPayload::valid(&public).with(7, |out| codec::encode_uint(out, 0)),
                InviteError::Invalid,
            ),
            (
                RawPayload::valid(&public).with(8, |out| codec::encode_uint(out, 1)),
                InviteError::Invalid,
            ),
            (
                RawPayload::valid(&public).with(0, |out| out.extend_from_slice(&[0x18, 0x01])),
                InviteError::Invalid,
            ),
        ];
        for (index, (payload, expected)) in cases.into_iter().enumerate() {
            assert_eq!(decode(payload), Err(expected), "case {index}");
        }
    }

    #[test]
    fn expired_forgeries_report_the_signature_first() {
        let binary = signed(Some(2_000_000_000)).canonical_binary().unwrap();
        let mut forged = binary.to_vec();
        let last = forged.len() - 1;
        forged[last] ^= 1;
        assert_eq!(
            SignedInviteCapsule::decode(&forged, Some(u64::MAX)),
            Err(InviteError::InvalidSignature)
        );
    }

    #[test]
    fn debug_output_redacts_the_access_code() {
        let debug = format!("{:?}", signed(None));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(CODE));
    }
}
