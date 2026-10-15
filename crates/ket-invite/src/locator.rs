//! Control-plane locators carried by an invite.

use std::collections::HashSet;

use url::{Host, Url};

use crate::{
    InviteError,
    codec::{self, Decoder},
};

pub const MAX_LOCATORS: usize = 4;
const MAX_HOST_BYTES: usize = 253;
const TAG_HTTPS: u64 = 1;
const TAG_LOOPBACK_DEVELOPMENT: u64 = 2;
const LOCATOR_FIELDS: usize = 3;

/// Suffixes that never name a public control endpoint.
const NON_PUBLIC_SUFFIXES: &[&str] = &[
    "local",
    "localhost",
    "internal",
    "onion",
    "i2p",
    "home.arpa",
    "test",
    "example",
    "invalid",
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoopbackHost {
    Localhost,
    Ipv4,
    Ipv6,
    AndroidEmulator,
}

impl LoopbackHost {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Localhost => "localhost",
            Self::Ipv4 => "127.0.0.1",
            Self::Ipv6 => "::1",
            Self::AndroidEmulator => "10.0.2.2",
        }
    }

    fn tag(self) -> u64 {
        match self {
            Self::Localhost => 1,
            Self::Ipv4 => 2,
            Self::Ipv6 => 3,
            Self::AndroidEmulator => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self, InviteError> {
        match tag {
            1 => Ok(Self::Localhost),
            2 => Ok(Self::Ipv4),
            3 => Ok(Self::Ipv6),
            4 => Ok(Self::AndroidEmulator),
            _ => Err(InviteError::UnsafeLocator),
        }
    }
}

/// Where a client reaches the node's control API.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ControlLocator {
    /// `https://<host>[:port]/`, where `host` is a public DNS name.
    Https { host: String, port: u16 },
    /// `http://<loopback>:<port>/` for local development only.
    LoopbackDevelopment { host: LoopbackHost, port: u16 },
}

impl ControlLocator {
    pub fn https(host: impl Into<String>, port: u16) -> Result<Self, InviteError> {
        let locator = Self::Https {
            host: host.into(),
            port,
        };
        locator.validate()?;
        Ok(locator)
    }

    /// Derives a locator from a server's public control URL. The URL must have
    /// no credentials, query, fragment, or path beyond `/`.
    pub fn from_public_url(value: &str) -> Result<Self, InviteError> {
        let url = Url::parse(value).map_err(|_| InviteError::UnsafeLocator)?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(InviteError::UnsafeLocator);
        }
        let port = url
            .port_or_known_default()
            .ok_or(InviteError::UnsafeLocator)?;
        let locator = match (url.scheme(), url.host()) {
            ("https", Some(Host::Domain(host))) => Self::Https {
                host: host.to_owned(),
                port,
            },
            ("http", Some(Host::Domain("localhost"))) => Self::LoopbackDevelopment {
                host: LoopbackHost::Localhost,
                port,
            },
            ("http", Some(Host::Ipv4(address))) if address.is_loopback() => {
                Self::LoopbackDevelopment {
                    host: LoopbackHost::Ipv4,
                    port,
                }
            }
            ("http", Some(Host::Ipv6(address))) if address.is_loopback() => {
                Self::LoopbackDevelopment {
                    host: LoopbackHost::Ipv6,
                    port,
                }
            }
            _ => return Err(InviteError::UnsafeLocator),
        };
        locator.validate()?;
        Ok(locator)
    }

    /// The control API base URL, always ending in `/`.
    pub fn base_url(&self) -> String {
        match self {
            Self::Https { host, port: 443 } => format!("https://{host}/"),
            Self::Https { host, port } => format!("https://{host}:{port}/"),
            Self::LoopbackDevelopment {
                host: LoopbackHost::Ipv6,
                port,
            } => format!("http://[::1]:{port}/"),
            Self::LoopbackDevelopment { host, port } => format!("http://{}:{port}/", host.as_str()),
        }
    }

    pub fn is_development(&self) -> bool {
        matches!(self, Self::LoopbackDevelopment { .. })
    }

    pub(crate) fn validate(&self) -> Result<(), InviteError> {
        match self {
            Self::Https { host, port } if *port != 0 && valid_public_dns_name(host) => Ok(()),
            Self::LoopbackDevelopment { port, .. } if *port != 0 => Ok(()),
            _ => Err(InviteError::UnsafeLocator),
        }
    }

    pub(crate) fn encode(&self, output: &mut Vec<u8>) {
        codec::encode_array(output, LOCATOR_FIELDS);
        match self {
            Self::Https { host, port } => {
                codec::encode_uint(output, TAG_HTTPS);
                codec::encode_text(output, host);
                codec::encode_uint(output, (*port).into());
            }
            Self::LoopbackDevelopment { host, port } => {
                codec::encode_uint(output, TAG_LOOPBACK_DEVELOPMENT);
                codec::encode_uint(output, host.tag());
                codec::encode_uint(output, (*port).into());
            }
        }
    }

    pub(crate) fn decode(decoder: &mut Decoder<'_>) -> Result<Self, InviteError> {
        decoder.array(LOCATOR_FIELDS)?;
        let locator = match decoder.uint()? {
            TAG_HTTPS => Self::Https {
                host: decoder.text(MAX_HOST_BYTES)?.to_owned(),
                port: decode_port(decoder)?,
            },
            TAG_LOOPBACK_DEVELOPMENT => Self::LoopbackDevelopment {
                host: LoopbackHost::from_tag(decoder.uint()?)?,
                port: decode_port(decoder)?,
            },
            _ => return Err(InviteError::UnsupportedLocator),
        };
        locator.validate()?;
        Ok(locator)
    }
}

pub(crate) fn validate_locator_set(locators: &[ControlLocator]) -> Result<(), InviteError> {
    if locators.is_empty() || locators.len() > MAX_LOCATORS {
        return Err(InviteError::Invalid);
    }
    let mut seen = HashSet::with_capacity(locators.len());
    for locator in locators {
        locator.validate()?;
        if !seen.insert(locator) {
            return Err(InviteError::Invalid);
        }
    }
    Ok(())
}

fn decode_port(decoder: &mut Decoder<'_>) -> Result<u16, InviteError> {
    u16::try_from(decoder.uint()?).map_err(|_| InviteError::UnsafeLocator)
}

/// A canonical lowercase public DNS name: two or more labels of `a-z`, `0-9`,
/// and inner hyphens, an alphabetic top-level label (which excludes IPv4
/// literals), and no reserved or private-use suffix.
fn valid_public_dns_name(host: &str) -> bool {
    if host.is_empty() || host.len() > MAX_HOST_BYTES {
        return false;
    }
    let labels = host.split('.').collect::<Vec<_>>();
    let valid_labels = labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        && labels
            .last()
            .is_some_and(|label| label.bytes().any(|byte| byte.is_ascii_lowercase()));
    valid_labels
        && !NON_PUBLIC_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(locator: &ControlLocator) -> ControlLocator {
        let mut output = Vec::new();
        locator.encode(&mut output);
        let mut decoder = Decoder::new(&output);
        let decoded = ControlLocator::decode(&mut decoder).unwrap();
        decoder.finish().unwrap();
        decoded
    }

    #[test]
    fn public_urls_map_to_canonical_locators_and_base_urls() {
        for (url, base) in [
            ("https://ket.example.com/", "https://ket.example.com/"),
            (
                "https://ket-1-2-3-4.sslip.io",
                "https://ket-1-2-3-4.sslip.io/",
            ),
            (
                "https://KET.Example.com:8443/",
                "https://ket.example.com:8443/",
            ),
            ("http://127.0.0.1:8787", "http://127.0.0.1:8787/"),
            ("http://localhost:8787/", "http://localhost:8787/"),
            ("http://[::1]:8787/", "http://[::1]:8787/"),
        ] {
            let locator = ControlLocator::from_public_url(url).unwrap();
            assert_eq!(locator.base_url(), base, "{url}");
            assert_eq!(round_trip(&locator), locator);
        }
        assert!(
            ControlLocator::from_public_url("http://127.0.0.1:8787")
                .unwrap()
                .is_development()
        );
    }

    #[test]
    fn unsafe_public_urls_are_rejected() {
        for url in [
            "http://ket.example.com/",
            "https://203.0.113.9/",
            "https://[2001:db8::1]/",
            "https://user:pass@ket.example.com/",
            "https://ket.example.com/prefix",
            "https://ket.example.com/?x=1",
            "https://ket.example.com/#x",
            "https://localhost/",
            "https://printer.local/",
            "https://router.home.arpa/",
            "https://svc.internal/",
            "https://node.test/",
            "https://single/",
            "http://10.0.0.1:8787/",
            "ftp://ket.example.com/",
            "not a url",
        ] {
            assert!(ControlLocator::from_public_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn decoded_locators_are_revalidated() {
        for host in [
            "Ket.example.com",
            "ket.example.com.",
            "-ket.example.com",
            "ket..example.com",
            "10.0.0.1",
            "ket.onion",
            &format!("{}.com", "a".repeat(64)),
        ] {
            let mut output = Vec::new();
            codec::encode_array(&mut output, 3);
            codec::encode_uint(&mut output, TAG_HTTPS);
            codec::encode_text(&mut output, host);
            codec::encode_uint(&mut output, 443);
            assert_eq!(
                ControlLocator::decode(&mut Decoder::new(&output)),
                Err(InviteError::UnsafeLocator),
                "{host}"
            );
        }

        let mut zero_port = Vec::new();
        ControlLocator::Https {
            host: "ket.example.com".to_owned(),
            port: 0,
        }
        .encode(&mut zero_port);
        assert_eq!(
            ControlLocator::decode(&mut Decoder::new(&zero_port)),
            Err(InviteError::UnsafeLocator)
        );

        let mut unknown = Vec::new();
        codec::encode_array(&mut unknown, 3);
        codec::encode_uint(&mut unknown, 9);
        codec::encode_uint(&mut unknown, 0);
        codec::encode_uint(&mut unknown, 443);
        assert_eq!(
            ControlLocator::decode(&mut Decoder::new(&unknown)),
            Err(InviteError::UnsupportedLocator)
        );
    }

    #[test]
    fn locator_sets_are_non_empty_bounded_and_unique() {
        let locator = ControlLocator::https("ket.example.com", 443).unwrap();
        assert!(validate_locator_set(std::slice::from_ref(&locator)).is_ok());
        assert_eq!(validate_locator_set(&[]), Err(InviteError::Invalid));
        assert_eq!(
            validate_locator_set(&[locator.clone(), locator.clone()]),
            Err(InviteError::Invalid)
        );
        let many = (0..5)
            .map(|index| ControlLocator::https(format!("n{index}.example.com"), 443).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(validate_locator_set(&many), Err(InviteError::Invalid));
    }
}
