//! Canonical, signed Ket invite capsules.
//!
//! An invite turns untrusted text into a bounded, authenticated enrollment
//! target: control locators, an access code, and the node's Ed25519 key. The
//! crate owns no networking; clients use [`EnrollmentProof`] to check that an
//! enrollment response was signed by the node named in the invite.

mod capsule;
mod codec;
mod enrollment;
mod error;
mod locator;
mod signing;
mod text;
#[cfg(test)]
mod vectors;

pub use capsule::{
    ACCESS_CODE_BYTES, CONTROL_API_VERSION, INVITE_FORMAT_VERSION, InviteCapsuleV1,
    KET_APPLICATION_ID, MAX_BINARY_INVITE_BYTES, MAX_ENCODED_INVITE_TEXT_BYTES,
    SignedInviteCapsule,
};
pub use enrollment::{
    ENROLLMENT_NONCE_BYTES, ENROLLMENT_NONCE_HEADER, EnrollmentNonce, EnrollmentProof,
    NODE_SIGNATURE_HEADER,
};
pub use error::InviteError;
pub use locator::{ControlLocator, LoopbackHost, MAX_LOCATORS};
pub use signing::{NodePublicKey, NodeSigningKey};
pub use text::{INVITE_TEXT_PREFIX, decode_invite_text, encode_invite_text};
