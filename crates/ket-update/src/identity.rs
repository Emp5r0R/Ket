use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{UpdateError, Version};

/// The last release before apps named themselves in the `Ket-Client` header.
pub const LEGACY_CLIENT_VERSION: Version = Version::new(1, 0, 1);

const MAX_HEADER_LENGTH: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPlatform {
    Android,
    Linux,
    Macos,
    Windows,
}

impl ClientPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

impl FromStr for ClientPlatform {
    type Err = UpdateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "android" => Ok(Self::Android),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Macos),
            "windows" => Ok(Self::Windows),
            _ => Err(UpdateError::InvalidClientHeader),
        }
    }
}

impl fmt::Display for ClientPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Which app is asking, as far as the server can tell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIdentity {
    /// `None` for apps that do not name their platform (v1.0.1 and earlier).
    pub platform: Option<ClientPlatform>,
    pub version: Version,
}

impl ClientIdentity {
    pub const fn new(platform: ClientPlatform, version: Version) -> Self {
        Self {
            platform: Some(platform),
            version,
        }
    }

    /// The identity of an app that sent no usable header: it predates the
    /// header, so it is at most v1.0.1 on an unknown platform.
    pub const fn legacy() -> Self {
        Self {
            platform: None,
            version: LEGACY_CLIENT_VERSION,
        }
    }

    /// Parses `platform/version`. Anything missing or malformed is treated as
    /// a legacy app rather than an error, so the header never becomes a way
    /// to probe the server.
    pub fn from_header(value: Option<&str>) -> Self {
        value
            .and_then(|value| Self::parse(value).ok())
            .unwrap_or_else(Self::legacy)
    }

    pub fn parse(value: &str) -> Result<Self, UpdateError> {
        if value.len() > MAX_HEADER_LENGTH {
            return Err(UpdateError::InvalidClientHeader);
        }
        let (platform, version) = value
            .split_once('/')
            .ok_or(UpdateError::InvalidClientHeader)?;
        Ok(Self::new(
            platform.parse()?,
            version
                .parse()
                .map_err(|_| UpdateError::InvalidClientHeader)?,
        ))
    }

    /// The header value an app sends.
    pub fn header_value(&self) -> Option<String> {
        self.platform
            .map(|platform| format!("{platform}/{}", self.version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_header() {
        let identity = ClientIdentity::new(ClientPlatform::Android, Version::new(1, 2, 0));
        let header = identity.header_value().unwrap();
        assert_eq!(header, "android/1.2.0");
        assert_eq!(ClientIdentity::from_header(Some(&header)), identity);
    }

    #[test]
    fn missing_or_malformed_headers_are_legacy_apps() {
        let long = format!("linux/{}", "1".repeat(80));
        for value in [
            None,
            Some(""),
            Some("android"),
            Some("ios/1.2.0"),
            Some("Android/1.2.0"),
            Some("linux/1.2"),
            Some("linux/v1.2.0"),
            Some("linux/1.2.0/extra"),
            Some(long.as_str()),
        ] {
            assert_eq!(
                ClientIdentity::from_header(value),
                ClientIdentity::legacy(),
                "{value:?}"
            );
        }
        assert_eq!(ClientIdentity::legacy().header_value(), None);
    }
}
