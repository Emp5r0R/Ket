//! Ket's update lifecycle: release versions, the release manifest that rates
//! each release's urgency for apps and servers, and the policy a server uses
//! to tell a connecting app whether it must, should, or may update.

mod category;
mod identity;
mod manifest;
mod policy;
mod version;

pub use category::UpdateCategory;
pub use identity::{ClientIdentity, ClientPlatform, LEGACY_CLIENT_VERSION};
pub use manifest::{Release, ReleaseManifest};
pub use policy::{ClientPolicy, Enforcement};
pub use version::Version;

/// The default page where people download Ket apps.
pub const DEFAULT_DOWNLOAD_URL: &str = "https://github.com/Emp5r0R/Ket/releases/latest";

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum UpdateError {
    #[error("{0:?} is not a MAJOR.MINOR.PATCH version")]
    InvalidVersion(String),
    #[error("the Ket-Client header is malformed")]
    InvalidClientHeader,
    #[error("the release manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("the client update policy is invalid: {0}")]
    InvalidPolicy(String),
}
