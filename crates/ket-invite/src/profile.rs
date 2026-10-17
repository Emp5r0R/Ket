//! Ket's profile of the shared invite format: an access-grant invite that
//! carries the 32-character access code and HTTPS or loopback control
//! locators.

use std::fmt;

use invite_capsule::{
    Capability, CapsuleError, InviteCapsule, InviteProfile, Locator, LocatorKinds, NodePublicKey,
    SignedInvite, cbor,
};
use zeroize::Zeroizing;

pub const KET_APPLICATION_ID: &str = "org.ket.access";
/// The session API in the clear, with signed enrollment responses.
pub const CONTROL_API_VERSION: u16 = 1;
/// The session API sealed to the node's key (`POST /v1/sealed`). An invite
/// that offers it obliges the app to use it.
pub const SEALED_CONTROL_API_VERSION: u16 = 2;
pub const ACCESS_CODE_BYTES: usize = 32;
pub const INVITE_TEXT_PREFIX: &str = "ket:invite:";
/// Domain for the node fingerprints Ket shows people.
pub const NODE_FINGERPRINT_DOMAIN: &[u8] = b"KET-NODE-FINGERPRINT-V1";

const CAPABILITY_ACCESS_GRANT: u64 = 1;

/// Ket's invite profile.
pub enum KetInvite {}

impl InviteProfile for KetInvite {
    const APPLICATION_ID: &'static str = KET_APPLICATION_ID;
    const SIGNATURE_DOMAIN: &'static [u8] = b"KET-INVITE-CAPSULE-V1";
    const CAPABILITY_TYPE: u64 = CAPABILITY_ACCESS_GRANT;
    const LOCATORS: LocatorKinds = LocatorKinds::CLEARNET;
    const TEXT_PREFIX: &'static str = INVITE_TEXT_PREFIX;
    type Capability = AccessCode;
}

pub type InviteCapsuleV1 = InviteCapsule<KetInvite>;
pub type SignedInviteCapsule = SignedInvite<KetInvite>;

impl KetInvite {
    /// An invite for control API versions [`CONTROL_API_VERSION`] through
    /// [`SEALED_CONTROL_API_VERSION`]: older apps keep enrolling in the clear,
    /// newer ones must seal.
    pub fn capsule(
        node_public_key: NodePublicKey,
        locators: Vec<Locator>,
        access_code: &str,
        access_expires_at_epoch_seconds: Option<u64>,
    ) -> Result<InviteCapsuleV1, CapsuleError> {
        InviteCapsule::new(
            node_public_key,
            locators,
            AccessCode::new(access_code)?,
            CONTROL_API_VERSION,
            SEALED_CONTROL_API_VERSION,
            access_expires_at_epoch_seconds,
        )
    }

    /// The control locator for a server's public URL, if Ket can use it.
    pub fn locator_from_public_url(value: &str) -> Result<Locator, CapsuleError> {
        let locator = Locator::from_public_url(value)?;
        if !Self::LOCATORS.accepts(&locator) {
            return Err(CapsuleError::UnsupportedLocator);
        }
        Ok(locator)
    }
}

/// The bearer access code an invite carries: exactly 32 ASCII letters and
/// digits. Zeroized on drop and never printed.
#[derive(Clone, Eq, PartialEq)]
pub struct AccessCode(Zeroizing<String>);

impl AccessCode {
    pub fn new(code: &str) -> Result<Self, CapsuleError> {
        if code.len() != ACCESS_CODE_BYTES || !code.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(CapsuleError::Invalid);
        }
        Ok(Self(Zeroizing::new(code.to_owned())))
    }

    /// The code itself. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessCode(<redacted>)")
    }
}

impl Capability for AccessCode {
    fn encode(&self, output: &mut Vec<u8>) {
        cbor::encode_text(output, &self.0);
    }

    fn decode(decoder: &mut cbor::Decoder<'_>) -> Result<Self, CapsuleError> {
        Self::new(decoder.text(ACCESS_CODE_BYTES)?)
    }
}

/// `ket:invite:<base64url>`.
pub fn encode_invite_text(invite: &SignedInviteCapsule) -> Result<Zeroizing<String>, CapsuleError> {
    invite.to_text()
}

/// Decodes and fully verifies invite text. The prefix is optional; anything
/// other than exact canonical Base64URL fails.
pub fn decode_invite_text(
    input: &str,
    now_epoch_seconds: Option<u64>,
) -> Result<SignedInviteCapsule, CapsuleError> {
    SignedInviteCapsule::from_text(input, now_epoch_seconds)
}

/// The fingerprint Ket shows for a node key, such as `1A2B:3C4D:...`.
pub fn node_fingerprint(key: &NodePublicKey) -> String {
    key.fingerprint(NODE_FINGERPRINT_DOMAIN)
}

#[cfg(test)]
mod tests {
    use invite_capsule::NodeSigningKey;

    use super::*;
    use crate::test_support::CODE;

    fn signed(expiry: Option<u64>) -> SignedInviteCapsule {
        let key = NodeSigningKey::from_seed(&[0x11; 32]);
        let capsule = KetInvite::capsule(
            key.public_key(),
            vec![Locator::https("ket.example.com", 443).unwrap()],
            CODE,
            expiry,
        )
        .unwrap();
        SignedInviteCapsule::sign(capsule, &key).unwrap()
    }

    #[test]
    fn invites_carry_the_access_code_and_control_api_version() {
        let text = encode_invite_text(&signed(Some(2_000_000_000))).unwrap();
        assert!(text.starts_with(INVITE_TEXT_PREFIX));
        let bare = &text[INVITE_TEXT_PREFIX.len()..];
        let decoded = decode_invite_text(bare, Some(1_900_000_000)).unwrap();
        let capsule = decoded.capsule();
        assert_eq!(capsule.capability().expose(), CODE);
        assert!(capsule.supports_protocol(CONTROL_API_VERSION));
        assert!(capsule.supports_protocol(SEALED_CONTROL_API_VERSION));
        assert!(!capsule.supports_protocol(SEALED_CONTROL_API_VERSION + 1));
        assert_eq!(
            decode_invite_text(&text, Some(2_000_000_000)),
            Err(CapsuleError::Expired)
        );
    }

    #[test]
    fn access_codes_must_be_exactly_32_ascii_alphanumerics() {
        assert!(AccessCode::new(CODE).is_ok());
        for code in [
            "short",
            "Abcdefghijklmnopqrstuvwxyz0123456",
            "Abcdefghijklmnopqrstuvwxyz01234-",
            "Abcdefghijklmnopqrstuvwxyz01234é",
        ] {
            assert_eq!(AccessCode::new(code), Err(CapsuleError::Invalid), "{code}");
        }
    }

    #[test]
    fn debug_output_redacts_the_access_code() {
        let debug = format!("{:?}", signed(None));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(CODE));
    }

    #[test]
    fn only_https_and_loopback_public_urls_make_control_locators() {
        assert!(KetInvite::locator_from_public_url("https://ket.example.com").is_ok());
        assert!(KetInvite::locator_from_public_url("http://127.0.0.1:8787").is_ok());
        assert_eq!(
            KetInvite::locator_from_public_url(
                "http://ukeu3k5oycgaauneqgtnvselmt4yemvoilkln7jpvamvfx7dnkdq.b32.i2p"
            ),
            Err(CapsuleError::UnsupportedLocator)
        );
    }
}
