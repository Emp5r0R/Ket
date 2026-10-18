//! Where a nearby device asks to go: a host name or an address, and a port.

use std::{
    fmt,
    net::{IpAddr, Ipv6Addr},
};

use thiserror::Error;

/// DNS limits a name to 253 characters and each label to 63.
const MAX_NAME_LENGTH: usize = 253;
const MAX_LABEL_LENGTH: usize = 63;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Host {
    Name(String),
    Address(IpAddr),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub host: Host,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TargetError {
    #[error("not a host name or address")]
    Invalid,
    /// `localhost`, `printer.local`, and other names that only mean something
    /// on this device or its own network.
    #[error("a name for this device or its own network")]
    LocalName,
}

impl Host {
    /// A host as a nearby device wrote it: an address, an IPv6 address in
    /// brackets, or a DNS name. Names are compared and resolved in lowercase.
    pub fn parse(text: &str) -> Result<Self, TargetError> {
        if let Some(inner) = text
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            return inner
                .parse::<Ipv6Addr>()
                .map(|address| Self::Address(IpAddr::V6(address)))
                .map_err(|_| TargetError::Invalid);
        }
        if let Ok(address) = text.parse::<IpAddr>() {
            return Ok(Self::Address(address));
        }
        let name = text.strip_suffix('.').unwrap_or(text).to_ascii_lowercase();
        if name.is_empty() || name.len() > MAX_NAME_LENGTH {
            return Err(TargetError::Invalid);
        }
        let labels = name.split('.').collect::<Vec<_>>();
        if !labels.iter().all(|label| valid_label(label)) {
            return Err(TargetError::Invalid);
        }
        // A single label (`router`) is looked up on the local network, and
        // these suffixes are never names on the Internet.
        let last = labels[labels.len() - 1];
        if labels.len() == 1
            || matches!(
                last,
                "localhost" | "local" | "internal" | "lan" | "home" | "arpa"
            )
        {
            return Err(TargetError::LocalName);
        }
        // An all-digit last label would make `1.2.3` mean an address to some
        // resolvers.
        if last.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(TargetError::Invalid);
        }
        Ok(Self::Name(name))
    }
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LENGTH
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

impl fmt::Display for Host {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => formatter.write_str(name),
            Self::Address(IpAddr::V6(address)) => write!(formatter, "[{address}]"),
            Self::Address(address) => write!(formatter, "{address}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_addresses_parse() {
        assert_eq!(
            Host::parse("Example.COM."),
            Ok(Host::Name("example.com".to_owned()))
        );
        assert_eq!(
            Host::parse("_dmarc.mail-1.example.org"),
            Ok(Host::Name("_dmarc.mail-1.example.org".to_owned()))
        );
        assert_eq!(
            Host::parse("203.0.113.5"),
            Ok(Host::Address("203.0.113.5".parse().unwrap()))
        );
        assert_eq!(
            Host::parse("[2001:db8::1]"),
            Ok(Host::Address("2001:db8::1".parse().unwrap()))
        );
        assert_eq!(
            Host::parse("2001:db8::1"),
            Ok(Host::Address("2001:db8::1".parse().unwrap()))
        );
    }

    #[test]
    fn malformed_names_are_refused() {
        for text in [
            "",
            ".",
            "a..b",
            "-a.com",
            "a-.com",
            "exa mple.com",
            "exa/mple.com",
            "[203.0.113.5]",
            "[2001:db8::1",
            "1.2.3",
            "999.1.1.1",
            &format!("{}.com", "a".repeat(64)),
            &format!("{}.com", "a.".repeat(126)),
        ] {
            assert_eq!(Host::parse(text), Err(TargetError::Invalid), "{text:?}");
        }
    }

    #[test]
    fn names_that_only_mean_something_nearby_are_refused() {
        for text in [
            "localhost",
            "router",
            "app.localhost",
            "printer.local",
            "nas.lan",
            "box.home",
            "db.internal",
            "1.0.0.127.in-addr.arpa",
        ] {
            assert_eq!(Host::parse(text), Err(TargetError::LocalName), "{text:?}");
        }
    }

    #[test]
    fn hosts_print_as_they_go_in_a_url() {
        assert_eq!(
            Host::parse("[2001:db8::1]").unwrap().to_string(),
            "[2001:db8::1]"
        );
        assert_eq!(
            Host::parse("203.0.113.5").unwrap().to_string(),
            "203.0.113.5"
        );
        assert_eq!(
            Host::parse("example.com").unwrap().to_string(),
            "example.com"
        );
    }
}
