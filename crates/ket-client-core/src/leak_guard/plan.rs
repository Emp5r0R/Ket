//! What the Windows leak guard allows and blocks, as plain data so the policy
//! is tested on every platform. The Filtering Platform layer only installs it.
//!
//! Rules are evaluated highest weight first and the first match decides:
//! anything on the tunnel interface or loopback passes; the engines may reach
//! the Ket server directly; every other DNS, DNS-over-TLS, LLMNR, or NetBIOS
//! name lookup is blocked, which defeats Windows' parallel lookups on every
//! adapter (Smart Multi-Homed Name Resolution) and link-local IPv6 resolvers;
//! IPv6 beyond the local link is blocked because the bridge only routes IPv4,
//! except to IPv6 networks the user keeps outside the tunnel. Name lookups
//! stay blocked even to those, so DNS never leaves outside the tunnel.

use std::net::{IpAddr, Ipv6Addr};

use ket_tunnel_protocol::IpPrefix;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Family {
    V4,
    V6,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Permit,
    Block,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Condition {
    /// Traffic that leaves through the tunnel interface.
    TunnelInterface,
    Loopback,
    RemoteAddress(IpAddr),
    RemotePort(u16),
    RemoteIpv6Prefix(Ipv6Addr, u8),
    Everything,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Rule {
    pub family: Family,
    pub action: Action,
    pub condition: Condition,
    pub weight: u8,
}

/// DNS, DNS over TLS, LLMNR, and NetBIOS name service.
pub(crate) const NAME_LOOKUP_PORTS: [u16; 4] = [53, 853, 5355, 137];

const TUNNEL_WEIGHT: u8 = 15;
const LOOPBACK_WEIGHT: u8 = 14;
const SERVER_WEIGHT: u8 = 13;
const LOOKUP_BLOCK_WEIGHT: u8 = 12;
const LOCAL_IPV6_WEIGHT: u8 = 11;
const IPV6_BLOCK_WEIGHT: u8 = 10;

const FAMILIES: [Family; 2] = [Family::V4, Family::V6];

/// Rules that apply before the tunnel interface exists: name lookups and
/// IPv6 are already blocked, so nothing leaks while the bridge starts.
pub(crate) fn base_rules(servers: &[IpAddr], bypass: &[IpPrefix]) -> Vec<Rule> {
    let mut rules = Vec::new();
    for family in FAMILIES {
        rules.push(rule(
            family,
            Action::Permit,
            Condition::Loopback,
            LOOPBACK_WEIGHT,
        ));
        for port in NAME_LOOKUP_PORTS {
            rules.push(rule(
                family,
                Action::Block,
                Condition::RemotePort(port),
                LOOKUP_BLOCK_WEIGHT,
            ));
        }
    }
    for server in servers {
        let family = match server {
            IpAddr::V4(_) => Family::V4,
            IpAddr::V6(_) => Family::V6,
        };
        rules.push(rule(
            family,
            Action::Permit,
            Condition::RemoteAddress(*server),
            SERVER_WEIGHT,
        ));
    }
    let bypassed = bypass.iter().filter_map(|prefix| match prefix.address() {
        IpAddr::V6(address) => Some((address, prefix.length())),
        IpAddr::V4(_) => None,
    });
    for (prefix, length) in [
        (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),
        (Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),
    ]
    .into_iter()
    .chain(bypassed)
    {
        rules.push(rule(
            Family::V6,
            Action::Permit,
            Condition::RemoteIpv6Prefix(prefix, length),
            LOCAL_IPV6_WEIGHT,
        ));
    }
    rules.push(rule(
        Family::V6,
        Action::Block,
        Condition::Everything,
        IPV6_BLOCK_WEIGHT,
    ));
    rules
}

/// Rules added once the tunnel interface exists: it carries everything.
pub(crate) fn tunnel_rules() -> Vec<Rule> {
    FAMILIES
        .into_iter()
        .map(|family| {
            rule(
                family,
                Action::Permit,
                Condition::TunnelInterface,
                TUNNEL_WEIGHT,
            )
        })
        .collect()
}

fn rule(family: Family, action: Action, condition: Condition, weight: u8) -> Rule {
    Rule {
        family,
        action,
        condition,
        weight,
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    /// A connection attempt, judged the way the Filtering Platform does: the
    /// highest-weight matching rule of its family decides; no match permits.
    struct Attempt {
        family: Family,
        tunnel: bool,
        loopback: bool,
        remote: IpAddr,
        port: u16,
    }

    fn outside(remote: IpAddr, port: u16) -> Attempt {
        Attempt {
            family: match remote {
                IpAddr::V4(_) => Family::V4,
                IpAddr::V6(_) => Family::V6,
            },
            tunnel: false,
            loopback: false,
            remote,
            port,
        }
    }

    fn matches(condition: Condition, attempt: &Attempt) -> bool {
        match condition {
            Condition::TunnelInterface => attempt.tunnel,
            Condition::Loopback => attempt.loopback,
            Condition::RemoteAddress(address) => attempt.remote == address,
            Condition::RemotePort(port) => attempt.port == port,
            Condition::RemoteIpv6Prefix(prefix, length) => match attempt.remote {
                IpAddr::V6(remote) => {
                    let mask = u128::MAX << (128 - u32::from(length));
                    u128::from(remote) & mask == u128::from(prefix) & mask
                }
                IpAddr::V4(_) => false,
            },
            Condition::Everything => true,
        }
    }

    fn decide(rules: &[Rule], attempt: &Attempt) -> Action {
        rules
            .iter()
            .filter(|rule| rule.family == attempt.family && matches(rule.condition, attempt))
            .max_by_key(|rule| rule.weight)
            .map_or(Action::Permit, |rule| rule.action)
    }

    fn all_rules(servers: &[IpAddr]) -> Vec<Rule> {
        let mut rules = base_rules(servers, &[]);
        rules.extend(tunnel_rules());
        rules
    }

    const LAN_RESOLVER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    const LINK_LOCAL_RESOLVER: IpAddr = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
    const SERVER: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
    const SERVER_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 7));
    const PUBLIC_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111));

    #[test]
    fn lookups_outside_the_tunnel_are_blocked_on_every_family_and_port() {
        let rules = all_rules(&[SERVER]);
        for remote in [LAN_RESOLVER, LINK_LOCAL_RESOLVER, PUBLIC_V6] {
            for port in NAME_LOOKUP_PORTS {
                assert_eq!(
                    decide(&rules, &outside(remote, port)),
                    Action::Block,
                    "{remote} port {port}"
                );
            }
        }
    }

    #[test]
    fn the_tunnel_and_loopback_carry_everything() {
        let rules = all_rules(&[SERVER]);
        for remote in [LAN_RESOLVER, PUBLIC_V6] {
            let mut attempt = outside(remote, 53);
            attempt.tunnel = true;
            assert_eq!(decide(&rules, &attempt), Action::Permit);
            let mut attempt = outside(remote, 53);
            attempt.loopback = true;
            assert_eq!(decide(&rules, &attempt), Action::Permit);
        }
    }

    #[test]
    fn engines_reach_the_server_and_ordinary_ipv4_is_left_to_the_routes() {
        let rules = all_rules(&[SERVER, SERVER_V6]);
        assert_eq!(decide(&rules, &outside(SERVER, 443)), Action::Permit);
        assert_eq!(decide(&rules, &outside(SERVER_V6, 443)), Action::Permit);
        assert_eq!(decide(&rules, &outside(LAN_RESOLVER, 443)), Action::Permit);
    }

    #[test]
    fn ipv6_stays_on_the_local_link() {
        let rules = all_rules(&[SERVER]);
        assert_eq!(decide(&rules, &outside(PUBLIC_V6, 443)), Action::Block);
        assert_eq!(
            decide(&rules, &outside(LINK_LOCAL_RESOLVER, 445)),
            Action::Permit
        );
        let multicast = IpAddr::V6(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb));
        assert_eq!(decide(&rules, &outside(multicast, 5353)), Action::Permit);
    }

    #[test]
    fn lookups_are_blocked_before_the_tunnel_interface_exists() {
        let rules = base_rules(&[SERVER], &[]);
        assert_eq!(decide(&rules, &outside(LAN_RESOLVER, 53)), Action::Block);
        assert_eq!(decide(&rules, &outside(PUBLIC_V6, 443)), Action::Block);
        assert!(
            rules
                .iter()
                .all(|rule| rule.condition != Condition::TunnelInterface)
        );
    }

    #[test]
    fn the_tunnel_outranks_every_block() {
        let highest_block = base_rules(&[SERVER], &[])
            .iter()
            .filter(|rule| rule.action == Action::Block)
            .map(|rule| rule.weight)
            .max()
            .unwrap();
        assert!(
            tunnel_rules()
                .iter()
                .all(|rule| rule.weight > highest_block)
        );
        assert_eq!(tunnel_rules().len(), 2);
    }

    #[test]
    fn bypassed_ipv6_networks_are_reached_directly_but_never_for_lookups() {
        let bypass = ["2606:4700::/32".parse().unwrap()];
        let mut rules = base_rules(&[SERVER], &bypass);
        rules.extend(tunnel_rules());
        assert_eq!(decide(&rules, &outside(PUBLIC_V6, 443)), Action::Permit);
        for port in NAME_LOOKUP_PORTS {
            assert_eq!(decide(&rules, &outside(PUBLIC_V6, port)), Action::Block);
        }
        let elsewhere = IpAddr::V6(Ipv6Addr::new(0x2a00, 0x1450, 0, 0, 0, 0, 0, 1));
        assert_eq!(decide(&rules, &outside(elsewhere, 443)), Action::Block);
    }
}
