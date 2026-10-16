//! Stable, non-secret rejection reasons.

use std::fmt;

/// Every rejection is fail-closed and carries a stable code. No variant ever
/// holds invite, capability, or key material.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CapsuleError {
    Invalid,
    TooLarge,
    UnsupportedVersion,
    WrongApplication,
    UnsupportedCapability,
    InvalidSignature,
    Expired,
    UnsupportedLocator,
    UnsafeLocator,
    InvalidNodeKey,
    NodeIdentityMismatch,
    InvalidChecksum,
}

impl CapsuleError {
    /// A stable snake_case code for clients and logs.
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
            Self::InvalidChecksum => "invalid_invite_checksum",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Invalid => "invalid invite",
            Self::TooLarge => "invite is too large",
            Self::UnsupportedVersion => "unsupported invite version",
            Self::WrongApplication => "invite belongs to another application",
            Self::UnsupportedCapability => "unsupported invite capability",
            Self::InvalidSignature => "invite signature is invalid",
            Self::Expired => "invite has expired",
            Self::UnsupportedLocator => "unsupported node locator",
            Self::UnsafeLocator => "unsafe node locator",
            Self::InvalidNodeKey => "invalid node key",
            Self::NodeIdentityMismatch => "node identity does not match the invite",
            Self::InvalidChecksum => "invite checksum is invalid",
        }
    }
}

impl fmt::Display for CapsuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for CapsuleError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_snake_case() {
        let all = [
            CapsuleError::Invalid,
            CapsuleError::TooLarge,
            CapsuleError::UnsupportedVersion,
            CapsuleError::WrongApplication,
            CapsuleError::UnsupportedCapability,
            CapsuleError::InvalidSignature,
            CapsuleError::Expired,
            CapsuleError::UnsupportedLocator,
            CapsuleError::UnsafeLocator,
            CapsuleError::InvalidNodeKey,
            CapsuleError::NodeIdentityMismatch,
            CapsuleError::InvalidChecksum,
        ];
        let codes = all
            .iter()
            .map(|error| error.code())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(codes.len(), all.len());
        for code in codes {
            assert!(
                code.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{code}"
            );
        }
    }
}
