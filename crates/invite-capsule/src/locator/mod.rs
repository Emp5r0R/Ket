//! Where a client reaches a node. Locators are signed data: parsing one never
//! resolves a name or opens a connection.

mod host;
mod private;

use std::collections::HashSet;

use crate::{
    CapsuleError,
    cbor::{self, Decoder},
};

pub use host::MAX_HOST_BYTES;

pub const MAX_LOCATORS: usize = 4;

const LOCATOR_FIELDS: usize = 3;
const TAG_HTTPS: u64 = 1;
const TAG_LOOPBACK_DEVELOPMENT: u64 = 2;
const TAG_ONION_V3: u64 = 3;
const TAG_I2P_B32: u64 = 4;

/// The loopback addresses a development locator may name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoopbackHost {
    Localhost,
    Ipv4,
    Ipv6,
    /// `10.0.2.2`, the host machine as seen from the Android emulator.
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

    fn from_tag(tag: u64) -> Result<Self, CapsuleError> {
        match tag {
            1 => Ok(Self::Localhost),
            2 => Ok(Self::Ipv4),
            3 => Ok(Self::Ipv6),
            4 => Ok(Self::AndroidEmulator),
            _ => Err(CapsuleError::UnsafeLocator),
        }
    }
}

/// One way to reach a node.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Locator {
    /// `https://<host>[:port]`, where `host` is a canonical public DNS name.
    Https { host: String, port: u16 },
    /// `http://<loopback>:<port>` for local development only.
    LoopbackDevelopment { host: LoopbackHost, port: u16 },
    /// A Tor v3 onion service, by its Ed25519 identity key.
    OnionV3 {
        service_identity: [u8; 32],
        port: u16,
    },
    /// An I2P destination, by its 32-byte hash.
    I2pB32 {
        destination_hash: [u8; 32],
        port: u16,
    },
}

impl Locator {
    pub fn https(host: impl Into<String>, port: u16) -> Result<Self, CapsuleError> {
        let locator = Self::Https {
            host: host.into(),
            port,
        };
        locator.validate()?;
        Ok(locator)
    }

    /// Derives a locator from a node's public URL. The URL must have no
    /// credentials, query, fragment, or path beyond `/`.
    pub fn from_public_url(value: &str) -> Result<Self, CapsuleError> {
        host::locator_from_public_url(value)
    }

    /// `scheme://authority`, with the default port omitted and no trailing
    /// slash.
    pub fn origin(&self) -> String {
        format!("{}://{}", self.scheme("https", "http"), self.display_host())
    }

    /// [`Self::origin`] followed by `/`.
    pub fn base_url(&self) -> String {
        format!("{}/", self.origin())
    }

    /// The WebSocket origin for the same authority (`wss` or `ws`).
    pub fn websocket_origin(&self) -> String {
        format!("{}://{}", self.scheme("wss", "ws"), self.display_host())
    }

    /// The authority (`host[:port]`), bracketing IPv6 and omitting the
    /// default port.
    pub fn display_host(&self) -> String {
        match self {
            Self::Https { host, port } => authority(host, *port, 443),
            Self::LoopbackDevelopment { host, port } => authority(host.as_str(), *port, 80),
            Self::OnionV3 {
                service_identity,
                port,
            } => authority(&private::onion_host(service_identity), *port, 80),
            Self::I2pB32 {
                destination_hash,
                port,
            } => authority(&private::i2p_host(destination_hash), *port, 80),
        }
    }

    pub fn is_development(&self) -> bool {
        matches!(self, Self::LoopbackDevelopment { .. })
    }

    pub fn validate(&self) -> Result<(), CapsuleError> {
        let valid = match self {
            Self::Https { host, port } => *port != 0 && host::valid_public_dns_name(host),
            Self::LoopbackDevelopment { port, .. } => *port != 0,
            Self::OnionV3 {
                service_identity,
                port,
            } => *port != 0 && private::valid_onion_key(service_identity),
            Self::I2pB32 {
                destination_hash,
                port,
            } => *port != 0 && *destination_hash != [0; 32],
        };
        valid.then_some(()).ok_or(CapsuleError::UnsafeLocator)
    }

    pub fn encode(&self, output: &mut Vec<u8>) {
        cbor::encode_array(output, LOCATOR_FIELDS);
        cbor::encode_uint(output, self.tag());
        match self {
            Self::Https { host, port } => {
                cbor::encode_text(output, host);
                cbor::encode_uint(output, (*port).into());
            }
            Self::LoopbackDevelopment { host, port } => {
                cbor::encode_uint(output, host.tag());
                cbor::encode_uint(output, (*port).into());
            }
            Self::OnionV3 {
                service_identity,
                port,
            } => {
                cbor::encode_bytes(output, service_identity);
                cbor::encode_uint(output, (*port).into());
            }
            Self::I2pB32 {
                destination_hash,
                port,
            } => {
                cbor::encode_bytes(output, destination_hash);
                cbor::encode_uint(output, (*port).into());
            }
        }
    }

    /// Decodes and validates one locator. A kind outside `accepted` fails
    /// with [`CapsuleError::UnsupportedLocator`] before its body is read.
    pub fn decode(decoder: &mut Decoder<'_>, accepted: LocatorKinds) -> Result<Self, CapsuleError> {
        decoder.array(LOCATOR_FIELDS)?;
        let tag = decoder.uint()?;
        if !accepted.accepts_tag(tag) {
            return Err(CapsuleError::UnsupportedLocator);
        }
        let locator = match tag {
            TAG_HTTPS => Self::Https {
                host: decoder.text(MAX_HOST_BYTES)?.to_owned(),
                port: decode_port(decoder)?,
            },
            TAG_LOOPBACK_DEVELOPMENT => Self::LoopbackDevelopment {
                host: LoopbackHost::from_tag(decoder.uint()?)?,
                port: decode_port(decoder)?,
            },
            TAG_ONION_V3 => Self::OnionV3 {
                service_identity: exact_32(decoder)?,
                port: decode_port(decoder)?,
            },
            TAG_I2P_B32 => Self::I2pB32 {
                destination_hash: exact_32(decoder)?,
                port: decode_port(decoder)?,
            },
            _ => return Err(CapsuleError::UnsupportedLocator),
        };
        locator.validate()?;
        Ok(locator)
    }

    fn tag(&self) -> u64 {
        match self {
            Self::Https { .. } => TAG_HTTPS,
            Self::LoopbackDevelopment { .. } => TAG_LOOPBACK_DEVELOPMENT,
            Self::OnionV3 { .. } => TAG_ONION_V3,
            Self::I2pB32 { .. } => TAG_I2P_B32,
        }
    }

    fn scheme(&self, tls: &'static str, plain: &'static str) -> &'static str {
        match self {
            Self::Https { .. } => tls,
            // Onion and I2P authenticate the node themselves; development
            // loopback never leaves the machine.
            _ => plain,
        }
    }
}

/// The locator kinds an application accepts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocatorKinds {
    pub https: bool,
    pub loopback_development: bool,
    pub onion_v3: bool,
    pub i2p_b32: bool,
}

impl LocatorKinds {
    /// Public HTTPS plus development loopback.
    pub const CLEARNET: Self = Self {
        https: true,
        loopback_development: true,
        onion_v3: false,
        i2p_b32: false,
    };
    pub const ALL: Self = Self {
        https: true,
        loopback_development: true,
        onion_v3: true,
        i2p_b32: true,
    };

    pub fn accepts(self, locator: &Locator) -> bool {
        self.accepts_tag(locator.tag())
    }

    fn accepts_tag(self, tag: u64) -> bool {
        match tag {
            TAG_HTTPS => self.https,
            TAG_LOOPBACK_DEVELOPMENT => self.loopback_development,
            TAG_ONION_V3 => self.onion_v3,
            TAG_I2P_B32 => self.i2p_b32,
            _ => false,
        }
    }
}

/// A locator set is non-empty, bounded, unique, valid, and of accepted kinds.
pub fn validate_locator_set(
    locators: &[Locator],
    accepted: LocatorKinds,
) -> Result<(), CapsuleError> {
    if locators.is_empty() || locators.len() > MAX_LOCATORS {
        return Err(CapsuleError::Invalid);
    }
    let mut seen = HashSet::with_capacity(locators.len());
    for locator in locators {
        if !accepted.accepts(locator) {
            return Err(CapsuleError::UnsupportedLocator);
        }
        locator.validate()?;
        if !seen.insert(locator) {
            return Err(CapsuleError::Invalid);
        }
    }
    Ok(())
}

fn decode_port(decoder: &mut Decoder<'_>) -> Result<u16, CapsuleError> {
    u16::try_from(decoder.uint()?).map_err(|_| CapsuleError::UnsafeLocator)
}

fn exact_32(decoder: &mut Decoder<'_>) -> Result<[u8; 32], CapsuleError> {
    decoder
        .bytes(32)?
        .try_into()
        .map_err(|_| CapsuleError::Invalid)
}

fn authority(host: &str, port: u16, default_port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == default_port {
        host
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(locator: &Locator) -> Locator {
        let mut output = Vec::new();
        locator.encode(&mut output);
        let mut decoder = Decoder::new(&output);
        let decoded = Locator::decode(&mut decoder, LocatorKinds::ALL).unwrap();
        decoder.finish().unwrap();
        decoded
    }

    #[test]
    fn every_kind_round_trips_with_its_urls() {
        let onion = "pg6mmjiyjmcrsslvykfwnntlaru7p5svn6y2ymmju6nubxndf4pscryd.onion";
        let i2p = "ukeu3k5oycgaauneqgtnvselmt4yemvoilkln7jpvamvfx7dnkdq.b32.i2p";
        for (url, origin, websocket) in [
            (
                "https://node.example.com/",
                "https://node.example.com",
                "wss://node.example.com",
            ),
            (
                "https://NODE.Example.com:8443",
                "https://node.example.com:8443",
                "wss://node.example.com:8443",
            ),
            (
                "http://127.0.0.1:8787",
                "http://127.0.0.1:8787",
                "ws://127.0.0.1:8787",
            ),
            ("http://[::1]:8787", "http://[::1]:8787", "ws://[::1]:8787"),
            (
                "http://10.0.2.2:4020",
                "http://10.0.2.2:4020",
                "ws://10.0.2.2:4020",
            ),
            (
                &format!("http://{onion}"),
                &format!("http://{onion}"),
                &format!("ws://{onion}"),
            ),
            (
                &format!("http://{i2p}:8080"),
                &format!("http://{i2p}:8080"),
                &format!("ws://{i2p}:8080"),
            ),
        ] {
            let locator = Locator::from_public_url(url).unwrap();
            assert_eq!(locator.origin(), origin, "{url}");
            assert_eq!(locator.base_url(), format!("{origin}/"), "{url}");
            assert_eq!(locator.websocket_origin(), websocket, "{url}");
            assert_eq!(round_trip(&locator), locator, "{url}");
        }
    }

    #[test]
    fn decoded_hosts_are_revalidated() {
        for host in [
            "Node.example.com",
            "node.example.com.",
            "-node.example.com",
            "node..example.com",
            "10.0.0.1",
            "1.2.3",
            "node.onion",
            "home.arpa",
            &format!("{}.com", "a".repeat(64)),
        ] {
            let mut output = Vec::new();
            cbor::encode_array(&mut output, 3);
            cbor::encode_uint(&mut output, TAG_HTTPS);
            cbor::encode_text(&mut output, host);
            cbor::encode_uint(&mut output, 443);
            assert_eq!(
                Locator::decode(&mut Decoder::new(&output), LocatorKinds::ALL),
                Err(CapsuleError::UnsafeLocator),
                "{host}"
            );
        }
        for port in [0, 65_536] {
            let mut output = Vec::new();
            cbor::encode_array(&mut output, 3);
            cbor::encode_uint(&mut output, TAG_HTTPS);
            cbor::encode_text(&mut output, "node.example.com");
            cbor::encode_uint(&mut output, port);
            assert_eq!(
                Locator::decode(&mut Decoder::new(&output), LocatorKinds::ALL),
                Err(CapsuleError::UnsafeLocator),
                "{port}"
            );
        }
    }

    #[test]
    fn unknown_and_unaccepted_kinds_are_unsupported_before_their_body() {
        let mut unknown = Vec::new();
        cbor::encode_array(&mut unknown, 3);
        cbor::encode_uint(&mut unknown, 9);
        cbor::encode_uint(&mut unknown, 0);
        cbor::encode_uint(&mut unknown, 443);
        assert_eq!(
            Locator::decode(&mut Decoder::new(&unknown), LocatorKinds::ALL),
            Err(CapsuleError::UnsupportedLocator)
        );
        // An onion body that would be unsafe still reads as unsupported to an
        // application that does not accept onion locators.
        let mut onion = Vec::new();
        cbor::encode_array(&mut onion, 3);
        cbor::encode_uint(&mut onion, TAG_ONION_V3);
        cbor::encode_bytes(&mut onion, &[0; 32]);
        cbor::encode_uint(&mut onion, 80);
        assert_eq!(
            Locator::decode(&mut Decoder::new(&onion), LocatorKinds::CLEARNET),
            Err(CapsuleError::UnsupportedLocator)
        );
        assert_eq!(
            Locator::decode(&mut Decoder::new(&onion), LocatorKinds::ALL),
            Err(CapsuleError::UnsafeLocator)
        );
    }

    #[test]
    fn locator_sets_are_non_empty_bounded_unique_and_accepted() {
        let locator = Locator::https("node.example.com", 443).unwrap();
        let accepted = LocatorKinds::CLEARNET;
        assert!(validate_locator_set(std::slice::from_ref(&locator), accepted).is_ok());
        assert_eq!(
            validate_locator_set(&[], accepted),
            Err(CapsuleError::Invalid)
        );
        assert_eq!(
            validate_locator_set(&[locator.clone(), locator.clone()], accepted),
            Err(CapsuleError::Invalid)
        );
        let many = (0..5)
            .map(|index| Locator::https(format!("n{index}.example.com"), 443).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            validate_locator_set(&many, accepted),
            Err(CapsuleError::Invalid)
        );
        let i2p = Locator::I2pB32 {
            destination_hash: [7; 32],
            port: 80,
        };
        assert_eq!(
            validate_locator_set(&[i2p], accepted),
            Err(CapsuleError::UnsupportedLocator)
        );
    }
}
