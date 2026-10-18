//! Which way each destination kept outside the tunnel should leave the
//! device, as plain data so the policy is tested on every platform. The
//! platform modules only read the routing table and install what this
//! decides.
//!
//! The full-route bridge sends everything into the tunnel interface. The
//! engines must still reach the Ket server directly, and the user's bypass
//! list must not use the tunnel, so each such destination gets a route
//! through the current physical default route. When the device changes
//! networks (Wi-Fi to Ethernet, a new hotspot), that default route changes
//! and these routes follow it, so the tunnel stays up and only the engine's
//! connections are redialled. With no physical network at all, a destination
//! is made unreachable instead: falling back into the tunnel would loop the
//! engine's traffic through itself, or send bypassed traffic through the
//! tunnel. A destination inside a network the device is directly attached to
//! (the home network, say) is left to that network's own route.

use std::{collections::BTreeMap, net::IpAddr};

use ket_tunnel_protocol::IpPrefix;

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

/// A network the device is directly attached to, without a gateway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OnLinkNetwork {
    pub prefix: IpPrefix,
    pub interface: String,
}

/// Where one destination is sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServerPath {
    Via(DefaultRoute),
    Unreachable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RouteAction {
    /// Installs `path` for `destination`, taking the place of `replacing`.
    Install {
        destination: IpPrefix,
        path: ServerPath,
        replacing: Option<ServerPath>,
    },
    Remove {
        destination: IpPrefix,
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

/// What to change so every destination follows the current default route,
/// or is left to the directly attached network that holds it.
pub(crate) fn plan(
    routes: &[DefaultRoute],
    on_link: &[OnLinkNetwork],
    destinations: &[IpPrefix],
    applied: &BTreeMap<IpPrefix, ServerPath>,
    tunnel_interface: &str,
) -> Vec<RouteAction> {
    destinations
        .iter()
        .filter_map(|destination| {
            let current = applied.get(destination);
            let attached = on_link.iter().any(|network| {
                network.interface != tunnel_interface
                    && network.prefix.length() > 0
                    && network.prefix.contains(destination)
            });
            if attached {
                return current.map(|path| RouteAction::Remove {
                    destination: *destination,
                    path: path.clone(),
                });
            }
            let desired =
                best_default(routes, Family::of(&destination.address()), tunnel_interface)
                    .cloned()
                    .map_or(ServerPath::Unreachable, ServerPath::Via);
            (current != Some(&desired)).then(|| RouteAction::Install {
                destination: *destination,
                path: desired,
                replacing: current.cloned(),
            })
        })
        .collect()
}

/// Removes every route Ket installed.
pub(crate) fn release(applied: &BTreeMap<IpPrefix, ServerPath>) -> Vec<RouteAction> {
    applied
        .iter()
        .map(|(destination, path)| RouteAction::Remove {
            destination: *destination,
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

    fn ip(value: &str) -> IpPrefix {
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

        let first = plan(std::slice::from_ref(&wifi), &[], &servers, &applied, "ket0");
        assert_eq!(first.len(), 2);
        for action in &first {
            let RouteAction::Install {
                destination, path, ..
            } = action
            else {
                panic!("expected an install");
            };
            applied.insert(*destination, path.clone());
        }
        assert!(plan(std::slice::from_ref(&wifi), &[], &servers, &applied, "ket0").is_empty());

        let moved = plan(
            &[wifi.clone(), ethernet.clone()],
            &[],
            &servers,
            &applied,
            "ket0",
        );
        assert_eq!(
            moved[0],
            RouteAction::Install {
                destination: ip("203.0.113.5"),
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
        let actions = plan(
            &[tunnel, v6.clone()],
            &[],
            &servers,
            &BTreeMap::new(),
            "ket0",
        );
        assert_eq!(
            actions,
            [
                RouteAction::Install {
                    destination: ip("203.0.113.5"),
                    path: ServerPath::Unreachable,
                    replacing: None,
                },
                RouteAction::Install {
                    destination: ip("2001:db8::5"),
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
                    destination: ip("198.51.100.7"),
                    path: ServerPath::Unreachable,
                },
                RouteAction::Remove {
                    destination: ip("203.0.113.5"),
                    path: ServerPath::Via(wifi),
                },
            ]
        );
        assert!(release(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn networks_follow_the_default_route_unless_the_device_is_attached_to_them() {
        let wifi = route(Family::V4, "wlan0", "192.168.1.1", 600);
        let on_link = [
            OnLinkNetwork {
                prefix: ip("192.168.1.0/24"),
                interface: "wlan0".to_owned(),
            },
            // The tunnel's own network never counts.
            OnLinkNetwork {
                prefix: ip("198.18.0.0/15"),
                interface: "ket0".to_owned(),
            },
        ];
        let destinations = [ip("192.168.1.20"), ip("198.18.5.0/24"), ip("10.20.0.0/16")];
        let actions = plan(
            std::slice::from_ref(&wifi),
            &on_link,
            &destinations,
            &BTreeMap::new(),
            "ket0",
        );
        let installed = actions
            .iter()
            .map(|action| match action {
                RouteAction::Install { destination, .. } => destination.to_string(),
                RouteAction::Remove { .. } => panic!("nothing to remove"),
            })
            .collect::<Vec<_>>();
        assert_eq!(installed, ["198.18.5.0/24", "10.20.0.0/16"]);

        // Joining the network that holds a destination hands it back.
        let applied = BTreeMap::from([(ip("10.20.0.0/16"), ServerPath::Via(wifi.clone()))]);
        let office = OnLinkNetwork {
            prefix: ip("10.20.0.0/16"),
            interface: "eth0".to_owned(),
        };
        assert_eq!(
            plan(
                std::slice::from_ref(&wifi),
                std::slice::from_ref(&office),
                &[ip("10.20.0.0/16")],
                &applied,
                "ket0"
            ),
            [RouteAction::Remove {
                destination: ip("10.20.0.0/16"),
                path: ServerPath::Via(wifi),
            }]
        );
    }
}
