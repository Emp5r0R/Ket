//! Which local addresses can be shared on, and which tunnel the share goes
//! out through, as plain data so the rules are tested on every platform.

use std::net::Ipv4Addr;

use serde::Serialize;

use crate::NetworkKind;

/// The tunnel bridge's interface (`tun2proxy`) and OpenVPN's on Linux.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const LINUX_BRIDGE: &str = "ket0";
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const LINUX_OPENVPN: &str = "ketvpn0";
/// The bridge answers name lookups itself, with stand-in addresses.
pub(crate) const BRIDGE_RESOLVERS: &[Ipv4Addr] = &[Ipv4Addr::new(198, 18, 0, 1)];
/// OpenVPN's lookups go to these public resolvers through the tunnel, as the
/// system's own do (see `system_dns`).
pub(crate) const OPENVPN_RESOLVERS: &[Ipv4Addr] =
    &[Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)];

/// An address of this computer on a network nearby devices can join.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ShareNetwork {
    pub interface: String,
    pub address: Ipv4Addr,
    pub kind: NetworkKind,
}

/// One IPv4 address of a working, non-loopback interface, as the system
/// lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalAddress {
    pub interface: String,
    pub address: Ipv4Addr,
    pub kind: NetworkKind,
    pub tunnel: bool,
}

/// Where the share's connections are tied to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Binding {
    /// A Linux interface name (`SO_BINDTODEVICE`).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Device(&'static str),
    /// A Windows interface index (`IP_UNICAST_IF`).
    #[cfg_attr(not(windows), allow(dead_code))]
    Index(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Tunnel {
    pub binding: Binding,
    pub resolvers: &'static [Ipv4Addr],
}

/// Only private addresses (RFC 1918): sharing on a public address would
/// offer the share to the whole Internet.
pub(crate) fn is_private(address: Ipv4Addr) -> bool {
    address.is_private()
}

/// The networks to offer, Wi-Fi first, then Ethernet, then the rest (a
/// Windows Mobile Hotspot, for one), each address once. Mobile data is never
/// offered: carriers hand out private addresses too, and sharing there would
/// offer the share to the carrier's network.
pub(crate) fn share_networks(addresses: Vec<LocalAddress>) -> Vec<ShareNetwork> {
    let mut networks = addresses
        .into_iter()
        .filter(|address| {
            !address.tunnel && address.kind != NetworkKind::Cellular && is_private(address.address)
        })
        .map(|address| ShareNetwork {
            interface: address.interface,
            address: address.address,
            kind: address.kind,
        })
        .collect::<Vec<_>>();
    networks.sort_by_key(|network| {
        (
            rank(network.kind),
            network.interface.clone(),
            network.address,
        )
    });
    networks.dedup_by_key(|network| network.address);
    networks
}

fn rank(kind: NetworkKind) -> u8 {
    match kind {
        NetworkKind::Wifi => 0,
        NetworkKind::Ethernet => 1,
        _ => 2,
    }
}

/// The Linux tunnel, from which of Ket's interfaces is up: the bridge's for
/// every transport but OpenVPN, which has its own.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn linux_tunnel(is_up: impl Fn(&str) -> bool) -> Option<Tunnel> {
    if is_up(LINUX_BRIDGE) {
        Some(Tunnel {
            binding: Binding::Device(LINUX_BRIDGE),
            resolvers: BRIDGE_RESOLVERS,
        })
    } else if is_up(LINUX_OPENVPN) {
        Some(Tunnel {
            binding: Binding::Device(LINUX_OPENVPN),
            resolvers: OPENVPN_RESOLVERS,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(interface: &str, address: [u8; 4], kind: NetworkKind, tunnel: bool) -> LocalAddress {
        LocalAddress {
            interface: interface.to_owned(),
            address: Ipv4Addr::from(address),
            kind,
            tunnel,
        }
    }

    #[test]
    fn only_private_addresses_outside_the_tunnel_are_offered() {
        let networks = share_networks(vec![
            local("docker0", [172, 17, 0, 1], NetworkKind::Other, false),
            local("eth0", [192, 168, 1, 20], NetworkKind::Ethernet, false),
            local("wlan0", [10, 42, 0, 1], NetworkKind::Wifi, false),
            local("ket0", [10, 0, 0, 33], NetworkKind::Other, true),
            local("eth1", [203, 0, 113, 7], NetworkKind::Ethernet, false),
            local("wwan0", [10, 64, 3, 9], NetworkKind::Cellular, false),
            local("wlan0", [10, 42, 0, 1], NetworkKind::Wifi, false),
        ]);
        assert_eq!(
            networks
                .iter()
                .map(|network| (network.interface.as_str(), network.address.to_string()))
                .collect::<Vec<_>>(),
            [
                ("wlan0", "10.42.0.1".to_owned()),
                ("eth0", "192.168.1.20".to_owned()),
                ("docker0", "172.17.0.1".to_owned()),
            ]
        );
    }

    #[test]
    fn private_means_rfc_1918() {
        for address in [
            [10, 0, 0, 1],
            [172, 16, 0, 1],
            [172, 31, 255, 254],
            [192, 168, 137, 1],
        ] {
            assert!(is_private(Ipv4Addr::from(address)), "{address:?}");
        }
        for address in [
            [100, 64, 0, 1],
            [169, 254, 1, 1],
            [127, 0, 0, 1],
            [172, 32, 0, 1],
            [8, 8, 8, 8],
        ] {
            assert!(!is_private(Ipv4Addr::from(address)), "{address:?}");
        }
    }

    #[test]
    fn the_bridge_is_preferred_and_openvpn_uses_its_resolvers() {
        assert_eq!(linux_tunnel(|_| false), None);
        assert_eq!(
            linux_tunnel(|name| name == LINUX_OPENVPN),
            Some(Tunnel {
                binding: Binding::Device(LINUX_OPENVPN),
                resolvers: OPENVPN_RESOLVERS
            })
        );
        assert_eq!(
            linux_tunnel(|_| true),
            Some(Tunnel {
                binding: Binding::Device(LINUX_BRIDGE),
                resolvers: BRIDGE_RESOLVERS
            })
        );
    }
}
