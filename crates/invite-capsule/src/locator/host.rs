//! Host policy and public-URL parsing.

use url::{Host, Url};

use super::{Locator, LoopbackHost, private};
use crate::CapsuleError;

pub const MAX_HOST_BYTES: usize = 253;
const MAX_PUBLIC_URL_BYTES: usize = 512;

/// Suffixes that never name a public node.
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

/// A canonical lowercase public DNS name: two or more labels of `a-z`, `0-9`,
/// and inner hyphens, an alphabetic top-level label (which excludes IPv4
/// literals), and no reserved or private-use suffix.
pub(super) fn valid_public_dns_name(host: &str) -> bool {
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

pub(super) fn locator_from_public_url(value: &str) -> Result<Locator, CapsuleError> {
    if value.is_empty()
        || value.len() > MAX_PUBLIC_URL_BYTES
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_whitespace())
    {
        return Err(CapsuleError::UnsafeLocator);
    }
    let url = Url::parse(value).map_err(|_| CapsuleError::UnsafeLocator)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(CapsuleError::UnsafeLocator);
    }
    let host = url.host().ok_or(CapsuleError::UnsafeLocator)?;
    let port = url
        .port_or_known_default()
        .ok_or(CapsuleError::UnsafeLocator)?;
    let locator = match (url.scheme(), host) {
        (scheme, Host::Domain(domain)) if domain.ends_with(".onion") => {
            require_plain_http(scheme)?;
            Locator::OnionV3 {
                service_identity: private::parse_onion_host(domain)?,
                port,
            }
        }
        (scheme, Host::Domain(domain)) if domain.ends_with(".i2p") => {
            require_plain_http(scheme)?;
            Locator::I2pB32 {
                destination_hash: private::parse_i2p_host(domain)?,
                port,
            }
        }
        // Literal remote IPs are excluded: a DNS name stays subject to the
        // client's own address policy.
        ("https", Host::Domain(domain)) => Locator::Https {
            host: domain.to_owned(),
            port,
        },
        ("http", host) => Locator::LoopbackDevelopment {
            host: loopback(host)?,
            port,
        },
        ("https", _) => return Err(CapsuleError::UnsafeLocator),
        _ => return Err(CapsuleError::UnsupportedLocator),
    };
    locator.validate()?;
    Ok(locator)
}

/// Onion and I2P authenticate the node themselves; HTTPS on top of them is not
/// a supported locator.
fn require_plain_http(scheme: &str) -> Result<(), CapsuleError> {
    (scheme == "http")
        .then_some(())
        .ok_or(CapsuleError::UnsupportedLocator)
}

fn loopback(host: Host<&str>) -> Result<LoopbackHost, CapsuleError> {
    match host {
        Host::Domain("localhost") => Ok(LoopbackHost::Localhost),
        Host::Ipv4(address) if address.is_loopback() => Ok(LoopbackHost::Ipv4),
        Host::Ipv4(address) if address.octets() == [10, 0, 2, 2] => {
            Ok(LoopbackHost::AndroidEmulator)
        }
        Host::Ipv6(address) if address.is_loopback() => Ok(LoopbackHost::Ipv6),
        _ => Err(CapsuleError::UnsafeLocator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_and_ssrf_shaped_urls_are_rejected() {
        for value in [
            "http://node.example.com/",
            "https://203.0.113.9/",
            "https://[2001:db8::1]/",
            "https://127.0.0.1",
            "http://169.254.169.254",
            "http://192.168.1.1",
            "http://10.0.0.1:8787/",
            "https://user:pass@node.example.com/",
            "https://node.example.com@evil.test",
            "https://node.example.com/prefix",
            "https://node.example.com/?x=1",
            "https://node.example.com/#x",
            "https://localhost/",
            "https://printer.local/",
            "https://router.home.arpa/",
            "https://svc.internal/",
            "https://node.test/",
            "https://single/",
            "https://ex\u{00e4}mple.com",
            "https://node.onion",
            "https://site.i2p",
            "ftp://node.example.com/",
            "file:///etc/passwd",
            "data:text/plain,x",
            "javascript:alert(1)",
            " https://node.example.com",
            "",
            "not a url",
        ] {
            assert!(
                Locator::from_public_url(value).is_err(),
                "accepted {value:?}"
            );
        }
        assert!(Locator::from_public_url(&format!("https://{}.com", "a".repeat(600))).is_err());
    }

    #[test]
    fn public_dns_names_need_an_alphabetic_top_level_label() {
        assert!(valid_public_dns_name("ket-1-2-3-4.sslip.io"));
        assert!(valid_public_dns_name("node.example.com"));
        for host in ["1.2.3", "node.123", "example", "home.arpa", "a.home.arpa"] {
            assert!(!valid_public_dns_name(host), "{host}");
        }
    }
}
