//! What an application contributes to the shared format.

use std::fmt;

use crate::{CapsuleError, LocatorKinds, cbor::Decoder};

/// One application's use of the invite format.
///
/// The constants are part of the signed bytes: changing any of them makes
/// every invite issued under the old value invalid, so treat them as frozen
/// once invites are in use.
pub trait InviteProfile: 'static {
    /// A reverse-DNS application ID such as `org.example.app`, at most
    /// [`crate::MAX_APPLICATION_ID_BYTES`] bytes. Invites for any other ID are
    /// [`CapsuleError::WrongApplication`].
    const APPLICATION_ID: &'static str;
    /// The signature domain, unique to this application.
    const SIGNATURE_DOMAIN: &'static [u8];
    /// The capability type tag this application issues and accepts.
    const CAPABILITY_TYPE: u64;
    /// The locator kinds this application can use.
    const LOCATORS: LocatorKinds;
    /// The text form's prefix, such as `app:invite:`. Decoding accepts the
    /// text with or without it.
    const TEXT_PREFIX: &'static str;
    /// The optional hand-typeable form.
    const MANUAL_TEXT: Option<ManualText> = None;

    /// The secret the invite grants.
    type Capability: Capability;
}

/// A hand-typeable form: Crockford Base32 in dash-separated groups of five,
/// with a 4-byte checksum so typing mistakes are reported as such.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManualText {
    /// Such as `APP1-`; matched without regard to case.
    pub prefix: &'static str,
    /// Domain for `SHA-256(domain || binary)[..4]`.
    pub checksum_domain: &'static [u8],
}

/// The capability field of an invite, encoded as one canonical CBOR item.
///
/// Implementations hold a bearer secret: they must zeroize it on drop and
/// must not print it in [`fmt::Debug`].
pub trait Capability: Clone + Eq + fmt::Debug + Sized {
    fn encode(&self, output: &mut Vec<u8>);

    /// Reads and validates the field. A well-formed but unacceptable value is
    /// [`CapsuleError::Invalid`].
    fn decode(decoder: &mut Decoder<'_>) -> Result<Self, CapsuleError>;
}
