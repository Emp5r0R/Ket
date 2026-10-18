//! Addresses and networks the user keeps outside the tunnel, such as a work
//! network or a site that refuses VPN traffic.
//!
//! The list crosses the broker into the privileged tunnel service, so it is
//! validated when it is decoded: at most [`MAX_BYPASS_ENTRIES`] entries, each
//! a single address or a network no wider than a /8 (IPv4) or /16 (IPv6), so
//! a mistyped entry cannot take most of the Internet out of the tunnel.
//! Loopback, multicast, and unspecified addresses are refused because they
//! never use the tunnel anyway. The Android app applies the same rules; both
//! replay `tests/vectors/bypass-entries.json`.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

pub const MAX_BYPASS_ENTRIES: usize = 64;
const MIN_IPV4_PREFIX: u8 = 8;
const MIN_IPV6_PREFIX: u8 = 16;

/// An address with a prefix length; host bits are always zero.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IpPrefix {
    address: IpAddr,
    length: u8,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PrefixError {
    #[error("not an IP address or network")]
    Invalid,
    #[error("the network is wider than /{0}")]
    TooWide(u8),
    #[error("loopback, multicast, and unspecified addresses never use the tunnel")]
    Special,
    #[error("at most {MAX_BYPASS_ENTRIES} entries")]
    TooMany,
}

impl PrefixError {
    /// A stable name for apps to show their own wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::TooWide(_) => "too_wide",
            Self::Special => "special",
            Self::TooMany => "too_many",
        }
    }
}

impl IpPrefix {
    /// The address alone: a /32 or /128.
    pub fn host(address: IpAddr) -> Self {
        Self {
            address,
            length: full_length(&address),
        }
    }

    /// `address/length` with its host bits cleared; `None` when `length` is
    /// longer than the address.
    pub fn new(address: IpAddr, length: u8) -> Option<Self> {
        (length <= full_length(&address)).then(|| Self {
            address: mask(address, length),
            length,
        })
    }

    pub fn address(&self) -> IpAddr {
        self.address
    }

    pub fn length(&self) -> u8 {
        self.length
    }

    pub fn is_ipv4(&self) -> bool {
        self.address.is_ipv4()
    }

    /// True when every address of `other` is inside this network.
    pub fn contains(&self, other: &Self) -> bool {
        self.address.is_ipv4() == other.address.is_ipv4()
            && other.length >= self.length
            && mask(other.address, self.length) == self.address
    }

    /// The dotted network mask of an IPv4 prefix, as OpenVPN's `route` takes it.
    pub fn ipv4_netmask(&self) -> Option<Ipv4Addr> {
        self.is_ipv4().then(|| {
            Ipv4Addr::from(
                u32::MAX
                    .checked_shl(32 - u32::from(self.length))
                    .unwrap_or(0),
            )
        })
    }

    /// Parses an entry a person typed and checks it may be kept outside the
    /// tunnel.
    pub fn parse_bypass(text: &str) -> Result<Self, PrefixError> {
        let prefix = text.trim().parse::<Self>()?;
        prefix.check_bypass()?;
        Ok(prefix)
    }

    fn check_bypass(&self) -> Result<(), PrefixError> {
        let special = match self.address {
            IpAddr::V4(address) => {
                address.is_loopback()
                    || address.is_multicast()
                    || address.is_unspecified()
                    || address.is_broadcast()
            }
            IpAddr::V6(address) => {
                address.is_loopback() || address.is_multicast() || address.is_unspecified()
            }
        };
        let minimum = if self.is_ipv4() {
            MIN_IPV4_PREFIX
        } else {
            MIN_IPV6_PREFIX
        };
        if self.length < minimum {
            Err(PrefixError::TooWide(minimum))
        } else if special {
            Err(PrefixError::Special)
        } else {
            Ok(())
        }
    }
}

impl FromStr for IpPrefix {
    type Err = PrefixError;

    /// `203.0.113.5`, `203.0.113.0/24`, `2001:db8::/32`. Host bits are
    /// cleared, so `203.0.113.9/24` is `203.0.113.0/24`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (address, length) = match text.split_once('/') {
            Some((address, length)) => (address, Some(length)),
            None => (text, None),
        };
        let address = address
            .parse::<IpAddr>()
            .map_err(|_| PrefixError::Invalid)?;
        // An IPv4 address written as IPv6 is entered as IPv4 instead.
        if let IpAddr::V6(v6) = address
            && v6.to_ipv4_mapped().is_some()
        {
            return Err(PrefixError::Invalid);
        }
        let length = match length {
            None => full_length(&address),
            Some(digits)
                if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                digits.parse::<u8>().map_err(|_| PrefixError::Invalid)?
            }
            Some(_) => return Err(PrefixError::Invalid),
        };
        Self::new(address, length).ok_or(PrefixError::Invalid)
    }
}

impl fmt::Display for IpPrefix {
    /// A single address prints without its length.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.length == full_length(&self.address) {
            write!(formatter, "{}", self.address)
        } else {
            write!(formatter, "{}/{}", self.address, self.length)
        }
    }
}

impl Serialize for IpPrefix {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for IpPrefix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A validated, sorted, duplicate-free bypass list.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct BypassList(Vec<IpPrefix>);

impl BypassList {
    /// Fails on the first entry that may not be bypassed, or when there are
    /// too many.
    pub fn new(entries: impl IntoIterator<Item = IpPrefix>) -> Result<Self, PrefixError> {
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort();
        entries.dedup();
        if entries.len() > MAX_BYPASS_ENTRIES {
            return Err(PrefixError::TooMany);
        }
        for entry in &entries {
            entry.check_bypass()?;
        }
        Ok(Self(entries))
    }

    pub fn entries(&self) -> &[IpPrefix] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for BypassList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = Vec::<IpPrefix>::deserialize(deserializer)?;
        if entries.len() > MAX_BYPASS_ENTRIES {
            return Err(serde::de::Error::custom(PrefixError::TooMany));
        }
        Self::new(entries).map_err(serde::de::Error::custom)
    }
}

fn full_length(address: &IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

fn mask(address: IpAddr, length: u8) -> IpAddr {
    match address {
        IpAddr::V4(address) => {
            let bits =
                u32::from(address) & u32::MAX.checked_shl(32 - u32::from(length)).unwrap_or(0);
            IpAddr::V4(Ipv4Addr::from(bits))
        }
        IpAddr::V6(address) => {
            let bits =
                u128::from(address) & u128::MAX.checked_shl(128 - u32::from(length)).unwrap_or(0);
            IpAddr::V6(Ipv6Addr::from(bits))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn entries_match_the_shared_vectors() {
        let vectors: Value =
            serde_json::from_str(include_str!("../tests/vectors/bypass-entries.json")).unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let result = IpPrefix::parse_bypass(input);
            match (&case["entry"], &case["error"]) {
                (Value::String(entry), _) => {
                    assert_eq!(
                        result.map(|prefix| prefix.to_string()).as_deref(),
                        Ok(entry.as_str()),
                        "{input}"
                    )
                }
                (_, Value::String(code)) => {
                    assert_eq!(
                        result.map_err(|error| error.code()),
                        Err(code.as_str()),
                        "{input}"
                    )
                }
                _ => panic!("case {input} names neither an entry nor an error"),
            }
        }
    }

    #[test]
    fn networks_contain_their_addresses_and_narrower_networks() {
        let network: IpPrefix = "192.168.0.0/16".parse().unwrap();
        assert!(network.contains(&"192.168.1.0/24".parse().unwrap()));
        assert!(network.contains(&"192.168.4.2".parse().unwrap()));
        assert!(!network.contains(&"192.169.0.1".parse().unwrap()));
        assert!(!network.contains(&"10.0.0.0/8".parse().unwrap()));
        assert!(!network.contains(&"2001:db8::1".parse().unwrap()));
        assert_eq!(network.ipv4_netmask(), Some(Ipv4Addr::new(255, 255, 0, 0)));
        assert_eq!(
            IpPrefix::host("203.0.113.5".parse().unwrap()).ipv4_netmask(),
            Some(Ipv4Addr::BROADCAST)
        );
    }

    #[test]
    fn lists_are_sorted_deduplicated_bounded_and_checked_when_decoded() {
        let list: BypassList =
            serde_json::from_value(json!(["203.0.113.9/24", "198.51.100.1", "203.0.113.0/24"]))
                .unwrap();
        assert_eq!(
            serde_json::to_value(&list).unwrap(),
            json!(["198.51.100.1", "203.0.113.0/24"])
        );
        assert!(serde_json::from_value::<BypassList>(json!(["0.0.0.0/0"])).is_err());
        assert!(serde_json::from_value::<BypassList>(json!(["127.0.0.1"])).is_err());
        let too_many = (0..=MAX_BYPASS_ENTRIES)
            .map(|index| format!("10.0.{}.{}", index / 200, index % 200 + 1))
            .collect::<Vec<_>>();
        assert!(serde_json::from_value::<BypassList>(json!(too_many)).is_err());
    }
}
