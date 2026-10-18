//! Which addresses a nearby device may reach through the share.
//!
//! Only the public Internet. A nearby device must never use the share to
//! reach this computer itself (its loopback services, such as the tunnel
//! service's control port), the local network, or addresses reserved for
//! documentation or other special uses. The check runs on the address that is
//! actually dialled, after any name lookup, so a name that resolves to a
//! private address is refused too.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// An IPv4 network, for the ranges the tunnel's own resolver hands out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ipv4Range {
    network: Ipv4Addr,
    length: u8,
}

impl Ipv4Range {
    pub const fn new(network: Ipv4Addr, length: u8) -> Self {
        Self { network, length }
    }

    pub fn contains(&self, address: Ipv4Addr) -> bool {
        let mask = if self.length == 0 {
            0
        } else {
            u32::MAX << (32 - u32::from(self.length))
        };
        u32::from(address) & mask == u32::from(self.network) & mask
    }
}

/// IPv4 networks that are not the public Internet (IANA special-purpose
/// registry), broadcast included in 240.0.0.0/4.
const RESERVED_IPV4: [Ipv4Range; 14] = [
    Ipv4Range::new(Ipv4Addr::new(0, 0, 0, 0), 8),
    Ipv4Range::new(Ipv4Addr::new(10, 0, 0, 0), 8),
    Ipv4Range::new(Ipv4Addr::new(100, 64, 0, 0), 10),
    Ipv4Range::new(Ipv4Addr::new(127, 0, 0, 0), 8),
    Ipv4Range::new(Ipv4Addr::new(169, 254, 0, 0), 16),
    Ipv4Range::new(Ipv4Addr::new(172, 16, 0, 0), 12),
    Ipv4Range::new(Ipv4Addr::new(192, 0, 0, 0), 24),
    Ipv4Range::new(Ipv4Addr::new(192, 0, 2, 0), 24),
    Ipv4Range::new(Ipv4Addr::new(192, 168, 0, 0), 16),
    Ipv4Range::new(Ipv4Addr::new(198, 18, 0, 0), 15),
    Ipv4Range::new(Ipv4Addr::new(198, 51, 100, 0), 24),
    Ipv4Range::new(Ipv4Addr::new(203, 0, 113, 0), 24),
    Ipv4Range::new(Ipv4Addr::new(224, 0, 0, 0), 4),
    Ipv4Range::new(Ipv4Addr::new(240, 0, 0, 0), 4),
];

/// Where a dialled address came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    /// Written by the nearby device.
    Requested,
    /// Returned by the tunnel's resolver for a name the device asked for.
    Resolved,
}

/// `synthetic` is the range the tunnel's resolver answers from when it hands
/// out stand-in addresses that the tunnel maps back to names (the desktop
/// bridge's 198.18.0.0/15, Android's 240.0.0.0/4). Those are reachable only
/// when they come from that resolver.
pub fn allowed(address: IpAddr, origin: Origin, synthetic: Option<Ipv4Range>) -> bool {
    match address {
        IpAddr::V4(address) => {
            if origin == Origin::Resolved && synthetic.is_some_and(|range| range.contains(address))
            {
                return true;
            }
            !RESERVED_IPV4.iter().any(|range| range.contains(address))
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return allowed(IpAddr::V4(mapped), origin, synthetic);
            }
            public_ipv6(address)
        }
    }
}

/// Global unicast (2000::/3) without documentation (2001:db8::/32) or 6to4
/// (2002::/16), which can wrap a private IPv4 address.
fn public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] & 0xe000 == 0x2000
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        && segments[0] != 0x2002
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRIDGE: Option<Ipv4Range> = Some(Ipv4Range::new(Ipv4Addr::new(198, 18, 0, 0), 15));
    const ANDROID: Option<Ipv4Range> = Some(Ipv4Range::new(Ipv4Addr::new(240, 0, 0, 0), 4));

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn public_addresses_are_reachable() {
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700:4700::1111",
        ] {
            assert!(allowed(ip(address), Origin::Requested, None), "{address}");
        }
    }

    #[test]
    fn this_device_and_its_networks_are_not() {
        for address in [
            "0.0.0.0",
            "127.0.0.1",
            "10.1.2.3",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.251",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "2001:db8::1",
            "2002:c0a8:0101::1",
            "64:ff9b::a00:1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
        ] {
            assert!(
                !allowed(ip(address), Origin::Requested, BRIDGE),
                "{address}"
            );
            assert!(!allowed(ip(address), Origin::Resolved, None), "{address}");
        }
    }

    #[test]
    fn neighbours_of_reserved_ranges_are_reachable() {
        for address in [
            "172.15.255.255",
            "172.32.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "198.17.255.255",
            "198.20.0.0",
        ] {
            assert!(allowed(ip(address), Origin::Requested, None), "{address}");
        }
    }

    #[test]
    fn stand_in_addresses_count_only_when_the_tunnel_resolver_gave_them() {
        assert!(allowed(ip("198.18.0.7"), Origin::Resolved, BRIDGE));
        assert!(!allowed(ip("198.18.0.7"), Origin::Requested, BRIDGE));
        assert!(!allowed(ip("198.18.0.7"), Origin::Resolved, None));
        assert!(allowed(ip("240.0.0.9"), Origin::Resolved, ANDROID));
        assert!(!allowed(ip("240.0.0.9"), Origin::Requested, ANDROID));
        // The stand-in range never opens the private networks.
        assert!(!allowed(ip("192.168.1.1"), Origin::Resolved, ANDROID));
    }
}
