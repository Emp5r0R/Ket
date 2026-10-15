use thiserror::Error;

/// Every rejection is fail-closed and carries a stable, non-secret code.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InviteError {
    #[error("invalid invite")]
    Invalid,
    #[error("invite is too large")]
    TooLarge,
    #[error("unsupported invite version")]
    UnsupportedVersion,
    #[error("invite belongs to another application")]
    WrongApplication,
    #[error("unsupported invite capability")]
    UnsupportedCapability,
    #[error("invite signature is invalid")]
    InvalidSignature,
    #[error("invite has expired")]
    Expired,
    #[error("unsupported control locator")]
    UnsupportedLocator,
    #[error("unsafe control locator")]
    UnsafeLocator,
    #[error("invalid node key")]
    InvalidNodeKey,
    #[error("node identity does not match the invite")]
    NodeIdentityMismatch,
}

impl InviteError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_invite",
            Self::TooLarge => "invite_too_large",
            Self::UnsupportedVersion => "unsupported_invite_version",
            Self::WrongApplication => "wrong_application",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::InvalidSignature => "invalid_invite_signature",
            Self::Expired => "invite_expired",
            Self::UnsupportedLocator => "unsupported_locator",
            Self::UnsafeLocator => "unsafe_locator",
            Self::InvalidNodeKey => "invalid_node_key",
            Self::NodeIdentityMismatch => "node_identity_mismatch",
        }
    }
}
