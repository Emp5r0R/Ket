use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::UpdateError;

/// A Ket release version, `MAJOR.MINOR.PATCH` with no pre-release suffix.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for Version {
    type Err = UpdateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || UpdateError::InvalidVersion(value.chars().take(32).collect());
        let mut parts = value.split('.');
        let mut next = || -> Result<u32, UpdateError> {
            let part = parts.next().ok_or_else(invalid)?;
            // Leading zeros and signs are rejected so each version has one spelling.
            if part.is_empty()
                || part.len() > 9
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let version = Self::new(next()?, next()?, next()?);
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_numerically() {
        let parsed: Version = "1.10.0".parse().unwrap();
        assert_eq!(parsed, Version::new(1, 10, 0));
        assert_eq!(parsed.to_string(), "1.10.0");
        assert!(Version::new(1, 9, 9) < parsed);
        assert!(Version::new(1, 10, 0) < Version::new(2, 0, 0));
        assert!("0.0.0".parse::<Version>().is_ok());
    }

    #[test]
    fn rejects_every_other_spelling() {
        for value in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3-rc.1",
            "01.2.3",
            "1.2.-3",
            "1..3",
            " 1.2.3",
            "1.2.3 ",
            "1.2.x",
            "9999999999.0.0",
        ] {
            assert!(value.parse::<Version>().is_err(), "{value:?} was accepted");
        }
    }
}
