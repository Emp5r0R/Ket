//! Which way each Ket server address should leave the device, as plain data
//! so the policy is tested on every platform. The platform modules only read
//! the routing table and install what this decides.
//!
//! The full-route bridge sends everything into the tunnel interface. The
//! engines must still reach the Ket server directly, so each server address
//! gets a host route through the current physical default route. When the
//! device changes networks (Wi-Fi to Ethernet, a new hotspot), that default
//! route changes and the host routes follow it, so the tunnel stays up and
//! only the engine's connections are redialled. With no physical network at
//! all, a server address is made unreachable instead: falling back into the
//! tunnel would loop the engine's traffic through itself.

use std::{collections::BTreeMap, net::IpAddr};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Family {
    V4,
    V6,
}

impl Family {
    pub(crate) fn of(address: &IpAddr) -> Self {
        match address {
            IpAddr::V4(_) => Self::V4,
            IpAddr::V6(_) => Self::V6,
        }
    }
}

/// A default route in the system table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DefaultRoute {
    pub family: Family,
    pub interface: String,
    pub interface_index: u32,
    /// `None` for an on-link route, such as a point-to-point modem.
    pub gateway: Option<IpAddr>,
    /// Lower wins, as the system decides.
    pub metric: u32,
}

/// Where one server address is sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServerPath {
    Via(DefaultRoute),
    Unreachable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RouteAction {
    /// Installs `path` for `server`, taking the place of `replacing`.
    Install {
        server: IpAddr,
        path: ServerPath,
        replacing: Option<ServerPath>,
    },
    Remove {
        server: IpAddr,
        path: ServerPath,
    },
}

/// The physical default route the system would use for `family`: the
/// lowest metric outside the tunnel interface.
pub(crate) fn best_default<'a>(
    routes: &'a [DefaultRoute],
    family: Family,
    tunnel_interface: &str,
) -> Option<&'a DefaultRoute> {
    routes
        .iter()
        .filter(|route| route.family == family && route.interface != tunnel_interface)
        .min_by_key(|route| (route.metric, route.interface_index))
}

/// What to change so every server follows the current default route.
pub(crate) fn plan(
    routes: &[DefaultRoute],
    servers: &[IpAddr],
    applied: &BTreeMap<IpAddr, ServerPath>,
    tunnel_interface: &str,
) -> Vec<RouteAction> {
    servers
        .iter()
        .filter_map(|server| {
            let desired = best_default(routes, Family::of(server), tunnel_interface)
                .cloned()
                .map_or(ServerPath::Unreachable, ServerPath::Via);
            let current = applied.get(server);
            (current != Some(&desired)).then(|| RouteAction::Install {
                server: *server,
                path: desired,
                replacing: current.cloned(),
            })
        })
        .collect()
}

/// Removes every route Ket installed.
pub(crate) fn release(applied: &BTreeMap<IpAddr, ServerPath>) -> Vec<RouteAction> {
    applied
        .iter()
        .map(|(server, path)| RouteAction::Remove {
            server: *server,
            path: path.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(family: Family, interface: &str, gateway: &str, metric: u32) -> DefaultRoute {
        DefaultRoute {
            family,
            interface: interface.to_owned(),
            interface_index: metric,
            gateway: Some(gateway.parse().unwrap()),
            metric,
        }
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn servers_follow_the_lowest_metric_physical_route_never_the_tunnel() {
        let wifi = route(Family::V4, "wlan0", "192.168.1.1", 600);
        let ethernet = route(Family::V4, "eth0", "10.0.0.1", 100);
        let tunnel = route(Family::V4, "ket0", "198.18.0.1", 0);
        let routes = [wifi.clone(), tunnel, ethernet.clone()];
        assert_eq!(best_default(&routes, Family::V4, "ket0"), Some(&ethernet));
        assert_eq!(best_default(&routes, Family::V6, "ket0"), None);
        assert_eq!(
            best_default(std::slice::from_ref(&wifi), Family::V4, "ket0"),
            Some(&wifi)
        );
    }

    #[test]
    fn a_network_change_moves_every_server_and_nothing_else() {
        let wifi = route(Family::V4, "wlan0", "192.168.1.1", 600);
        let ethernet = route(Family::V4, "eth0", "10.0.0.1", 100);
        let servers = [ip("203.0.113.5"), ip("198.51.100.7")];
        let mut applied = BTreeMap::new();

        let first = plan(std::slice::from_ref(&wifi), &servers, &applied, "ket0");
        assert_eq!(first.len(), 2);
        for action in &first {
            let RouteAction::Install { server, path, .. } = action else {
                panic!("expected an install");
            };
            applied.insert(*server, path.clone());
        }
        assert!(plan(std::slice::from_ref(&wifi), &servers, &applied, "ket0").is_empty());

        let moved = plan(
            &[wifi.clone(), ethernet.clone()],
            &servers,
            &applied,
            "ket0",
        );
        assert_eq!(
            moved[0],
            RouteAction::Install {
                server: ip("203.0.113.5"),
                path: ServerPath::Via(ethernet),
                replacing: Some(ServerPath::Via(wifi)),
            }
        );
        assert_eq!(moved.len(), 2);
    }

    #[test]
    fn without_a_physical_network_servers_are_unreachable_not_looped() {
        let tunnel = route(Family::V4, "ket0", "198.18.0.1", 0);
        let v6 = route(Family::V6, "eth0", "fe80::1", 100);
        let servers = [ip("203.0.113.5"), ip("2001:db8::5")];
        let actions = plan(&[tunnel, v6.clone()], &servers, &BTreeMap::new(), "ket0");
        assert_eq!(
            actions,
            [
                RouteAction::Install {
                    server: ip("203.0.113.5"),
                    path: ServerPath::Unreachable,
                    replacing: None,
                },
                RouteAction::Install {
                    server: ip("2001:db8::5"),
                    path: ServerPath::Via(v6),
                    replacing: None,
                },
            ]
        );
    }

    #[test]
    fn release_removes_exactly_what_was_installed() {
        let wifi = route(Family::V4, "wlan0", "192.168.1.1", 600);
        let applied = BTreeMap::from([
            (ip("203.0.113.5"), ServerPath::Via(wifi.clone())),
            (ip("198.51.100.7"), ServerPath::Unreachable),
        ]);
        assert_eq!(
            release(&applied),
            [
                RouteAction::Remove {
                    server: ip("198.51.100.7"),
                    path: ServerPath::Unreachable,
                },
                RouteAction::Remove {
                    server: ip("203.0.113.5"),
                    path: ServerPath::Via(wifi),
                },
            ]
        );
        assert!(release(&BTreeMap::new()).is_empty());
    }
}
