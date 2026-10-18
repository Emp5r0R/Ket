//! The Linux routing table through `/proc/net` (read every check, no process
//! started) and `ip route` (only when something changes).

use std::{
    fs, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    process::{Command, Stdio},
};

use super::{DefaultRoute, Family, RouteAction, RouteSystem, ServerPath};

/// Marks Ket's routes (`proto 75`) so leftovers can be found and removed.
const ROUTE_PROTOCOL: &str = "75";
const ROUTE_METRIC: &str = "23";
const RTF_UP: u32 = 0x0001;
const RTF_GATEWAY: u32 = 0x0002;
const RTF_REJECT: u32 = 0x0200;
const IP_CANDIDATES: [&str; 4] = ["/usr/sbin/ip", "/sbin/ip", "/usr/bin/ip", "/bin/ip"];

pub(crate) struct LinuxRoutes {
    ip: &'static str,
}

impl LinuxRoutes {
    pub(crate) fn new() -> Self {
        Self {
            ip: IP_CANDIDATES
                .into_iter()
                .find(|path| Path::new(path).exists())
                .unwrap_or("ip"),
        }
    }

    fn ip(&self, arguments: &[String]) -> io::Result<()> {
        let output = Command::new(self.ip)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "ip {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

/// The default routes in the system table; reading needs no privileges.
pub(crate) fn read_default_routes() -> io::Result<Vec<DefaultRoute>> {
    let mut routes = parse_ipv4_routes(&fs::read_to_string("/proc/net/route")?);
    // Absent when IPv6 is disabled.
    if let Ok(table) = fs::read_to_string("/proc/net/ipv6_route") {
        routes.extend(parse_ipv6_routes(&table));
    }
    Ok(routes)
}

/// The hardware address of an IPv4 neighbour, such as the gateway, from
/// `/proc/net/arp`, which tells apart networks that share a gateway address.
pub(crate) fn neighbour_hardware(address: IpAddr) -> Option<String> {
    parse_neighbour(&fs::read_to_string("/proc/net/arp").ok()?, address)
}

fn parse_neighbour(table: &str, address: IpAddr) -> Option<String> {
    let wanted = address.to_string();
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.first() == Some(&wanted.as_str()) && fields.get(3) != Some(&"00:00:00:00:00:00"))
            .then(|| fields.get(3).map(|hardware| (*hardware).to_owned()))
            .flatten()
    })
}

impl RouteSystem for LinuxRoutes {
    fn default_routes(&mut self) -> io::Result<Vec<DefaultRoute>> {
        read_default_routes()
    }

    fn apply(&mut self, action: &RouteAction) -> io::Result<()> {
        self.ip(&arguments(action))
    }

    fn clear_stale(&mut self) -> io::Result<()> {
        self.ip(&strings(&["-4", "route", "flush", "proto", ROUTE_PROTOCOL]))?;
        // IPv6 may be disabled; nothing can be left there then.
        let _ = self.ip(&strings(&["-6", "route", "flush", "proto", ROUTE_PROTOCOL]));
        Ok(())
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// The `ip` arguments for one action.
fn arguments(action: &RouteAction) -> Vec<String> {
    let (verb, server, path) = match action {
        RouteAction::Install { server, path, .. } => ("replace", server, path),
        RouteAction::Remove { server, path } => ("del", server, path),
    };
    let (family, prefix) = match server {
        IpAddr::V4(_) => ("-4", 32),
        IpAddr::V6(_) => ("-6", 128),
    };
    let mut arguments = strings(&[family, "route", verb]);
    if matches!(path, ServerPath::Unreachable) {
        arguments.push("unreachable".to_owned());
    }
    arguments.push(format!("{server}/{prefix}"));
    if let (ServerPath::Via(route), "replace") = (path, verb) {
        if let Some(gateway) = route.gateway {
            arguments.extend(["via".to_owned(), gateway.to_string()]);
        }
        arguments.extend(["dev".to_owned(), route.interface.clone()]);
    }
    arguments.extend(strings(&["metric", ROUTE_METRIC, "proto", ROUTE_PROTOCOL]));
    arguments
}

/// Default routes in `/proc/net/route`: `Iface Destination Gateway Flags
/// RefCnt Use Metric Mask ...`, addresses in little-endian hex.
fn parse_ipv4_routes(table: &str) -> Vec<DefaultRoute> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let hex = |index: usize| u32::from_str_radix(fields.get(index)?, 16).ok();
            let (destination, gateway, flags, mask) = (hex(1)?, hex(2)?, hex(3)?, hex(7)?);
            if destination != 0 || mask != 0 || flags & RTF_UP == 0 || flags & RTF_REJECT != 0 {
                return None;
            }
            Some(DefaultRoute {
                family: Family::V4,
                interface: (*fields.first()?).to_owned(),
                interface_index: 0,
                gateway: (flags & RTF_GATEWAY != 0)
                    .then(|| IpAddr::V4(Ipv4Addr::from(gateway.swap_bytes()))),
                metric: fields.get(6)?.parse().ok()?,
            })
        })
        .collect()
}

/// Default routes in `/proc/net/ipv6_route`: `destination prefix source
/// prefix next_hop metric refcnt use flags interface`, addresses in hex.
fn parse_ipv6_routes(table: &str) -> Vec<DefaultRoute> {
    table
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                return None;
            }
            let destination = u128::from_str_radix(fields[0], 16).ok()?;
            let prefix = u8::from_str_radix(fields[1], 16).ok()?;
            let next_hop = u128::from_str_radix(fields[4], 16).ok()?;
            let metric = u32::from_str_radix(fields[5], 16).ok()?;
            let flags = u32::from_str_radix(fields[8], 16).ok()?;
            let interface = fields[9];
            if destination != 0
                || prefix != 0
                || flags & RTF_UP == 0
                || flags & RTF_REJECT != 0
                || interface == "lo"
            {
                return None;
            }
            Some(DefaultRoute {
                family: Family::V6,
                interface: interface.to_owned(),
                interface_index: 0,
                gateway: (next_hop != 0).then(|| IpAddr::V6(Ipv6Addr::from(next_hop))),
                metric,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPV4: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
ket0\t00000000\t00000000\t0001\t0\t0\t0\t00000080\t0\t0\t0
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
ppp0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\t0\t0\t0
";

    const IPV6: &str = "\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 wlan0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001 wlan0
";

    #[test]
    fn ipv4_default_routes_are_read_with_gateways_and_metrics() {
        let routes = parse_ipv4_routes(IPV4);
        assert_eq!(
            routes,
            [
                DefaultRoute {
                    family: Family::V4,
                    interface: "wlan0".to_owned(),
                    interface_index: 0,
                    gateway: Some("192.168.1.1".parse().unwrap()),
                    metric: 600,
                },
                DefaultRoute {
                    family: Family::V4,
                    interface: "ppp0".to_owned(),
                    interface_index: 0,
                    gateway: None,
                    metric: 50,
                },
            ],
            "the tunnel's half routes and subnet routes are not defaults"
        );
    }

    #[test]
    fn neighbours_are_found_by_address_and_incomplete_entries_skipped() {
        let table = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        wlan0
10.0.0.1         0x1         0x0         00:00:00:00:00:00     *        eth0
";
        assert_eq!(
            parse_neighbour(table, "192.168.1.1".parse().unwrap()).as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
        assert_eq!(parse_neighbour(table, "10.0.0.1".parse().unwrap()), None);
        assert_eq!(parse_neighbour(table, "10.0.0.2".parse().unwrap()), None);
    }

    #[test]
    fn ipv6_default_routes_skip_the_unreachable_loopback_default() {
        assert_eq!(
            parse_ipv6_routes(IPV6),
            [DefaultRoute {
                family: Family::V6,
                interface: "wlan0".to_owned(),
                interface_index: 0,
                gateway: Some("fe80::1".parse().unwrap()),
                metric: 0x400,
            }]
        );
    }

    #[test]
    fn actions_become_tagged_ip_route_commands() {
        let wifi = DefaultRoute {
            family: Family::V4,
            interface: "wlan0".to_owned(),
            interface_index: 0,
            gateway: Some("192.168.1.1".parse().unwrap()),
            metric: 600,
        };
        let server: IpAddr = "203.0.113.5".parse().unwrap();
        assert_eq!(
            arguments(&RouteAction::Install {
                server,
                path: ServerPath::Via(wifi.clone()),
                replacing: None,
            })
            .join(" "),
            "-4 route replace 203.0.113.5/32 via 192.168.1.1 dev wlan0 metric 23 proto 75"
        );
        assert_eq!(
            arguments(&RouteAction::Install {
                server: "2001:db8::5".parse().unwrap(),
                path: ServerPath::Unreachable,
                replacing: Some(ServerPath::Via(wifi.clone())),
            })
            .join(" "),
            "-6 route replace unreachable 2001:db8::5/128 metric 23 proto 75"
        );
        assert_eq!(
            arguments(&RouteAction::Remove {
                server,
                path: ServerPath::Via(wifi),
            })
            .join(" "),
            "-4 route del 203.0.113.5/32 metric 23 proto 75"
        );
    }
}
