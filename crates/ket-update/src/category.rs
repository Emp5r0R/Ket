use serde::{Deserialize, Serialize};

/// How urgent a release is for apps or for servers, from least to most.
///
/// - `optional`: new features; nothing to act on.
/// - `recommended`: bug fixes worth having.
/// - `security`: a security fix. Apps must update within the manifest's
///   grace period; servers with automatic updates apply it on their own.
/// - `critical`: must be applied now. Servers refuse older apps at once.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCategory {
    Optional,
    Recommended,
    Security,
    Critical,
}

impl UpdateCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Optional => "optional",
            Self::Recommended => "recommended",
            Self::Security => "security",
            Self::Critical => "critical",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_ordered_by_urgency_with_stable_names() {
        let ordered = [
            UpdateCategory::Optional,
            UpdateCategory::Recommended,
            UpdateCategory::Security,
            UpdateCategory::Critical,
        ];
        for pair in ordered.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        for category in ordered {
            assert_eq!(
                serde_json::to_value(category).unwrap(),
                serde_json::Value::from(category.as_str())
            );
        }
    }
}
