//! Canonical, signed invite capsules.
//!
//! An invite turns untrusted text into a bounded, authenticated target: where
//! to reach a node, the node's Ed25519 key, and an application-defined
//! capability. Every application that uses this format describes itself with
//! an [`InviteProfile`]: its application ID, signature domain, capability
//! type, accepted locator kinds, and text forms. The wire format, canonical
//! encoding, signature rules, and validation are shared, so two applications
//! never disagree about what a valid invite is, and an invite for one is
//! rejected by the other.
//!
//! The crate owns no networking, storage, or UI.

mod capsule;
pub mod cbor;
mod error;
mod locator;
mod node_key;
mod profile;
mod signature;
mod signed;
mod text;

pub use capsule::{INVITE_FORMAT_VERSION, InviteCapsule, MAX_APPLICATION_ID_BYTES};
pub use error::CapsuleError;
pub use locator::{
    Locator, LocatorKinds, LoopbackHost, MAX_HOST_BYTES, MAX_LOCATORS, validate_locator_set,
};
pub use node_key::{NodePublicKey, NodeSigningKey, key_fingerprint};
pub use profile::{Capability, InviteProfile, ManualText};
pub use signature::signature_input;
pub use signed::{MAX_BINARY_INVITE_BYTES, MAX_ENCODED_INVITE_TEXT_BYTES, SignedInvite};
