//! Ket's signed invites, built on the shared `invite-capsule` format.
//!
//! An invite turns untrusted text into a bounded, authenticated enrollment
//! target: control locators, an access code, and the node's Ed25519 key. The
//! crate owns no networking; clients use [`EnrollmentProof`] to check that an
//! enrollment response was signed by the node named in the invite.

mod enrollment;
mod profile;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod vectors;

pub use enrollment::{
    ENROLLMENT_NONCE_BYTES, ENROLLMENT_NONCE_HEADER, EnrollmentNonce, EnrollmentProof,
    NODE_SIGNATURE_HEADER,
};
pub use invite_capsule::{
    CapsuleError as InviteError, INVITE_FORMAT_VERSION, Locator as ControlLocator, LoopbackHost,
    MAX_BINARY_INVITE_BYTES, MAX_ENCODED_INVITE_TEXT_BYTES, MAX_LOCATORS, NodePublicKey,
    NodeSigningKey,
};
pub use profile::{
    ACCESS_CODE_BYTES, AccessCode, CONTROL_API_VERSION, INVITE_TEXT_PREFIX, InviteCapsuleV1,
    KET_APPLICATION_ID, KetInvite, NODE_FINGERPRINT_DOMAIN, SignedInviteCapsule,
    decode_invite_text, encode_invite_text, node_fingerprint,
};
