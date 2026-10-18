//! The Windows routing table through the IP Helper API.
//!
//! Ket's routes carry metric [`ROUTE_METRIC`] and the network-management
//! protocol, so a later start can remove any a crashed process left behind.
//! Windows drops them at reboot either way, because they are not persistent.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr, slice,
};

use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
    NetworkManagement::{
        IpHelper::{
            ConvertInterfaceLuidToAlias, CreateIpForwardEntry2, DeleteIpForwardEntry2,
            FreeMibTable, GetIfTable2, GetIpForwardTable2, GetIpInterfaceEntry,
            IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_WWANPP,
            IF_TYPE_WWANPP2, InitializeIpForwardEntry, InitializeIpInterfaceEntry, MIB_IF_TABLE2,
            MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
        },
        Ndis::{IF_MAX_STRING_SIZE, NET_LUID_LH},
    },
    Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, IN_ADDR, IN_ADDR_0, IN6_ADDR, IN6_ADDR_0,
        MIB_IPPROTO_NETMGMT, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_INET,
    },
};

use ket_tunnel_protocol::IpPrefix;

use super::{
    DefaultRoute, Family, NetworkKind, OnLinkNetwork, RouteAction, RouteSystem, ServerPath,
};

const ROUTE_METRIC: u32 = 23;

pub(crate) struct WindowsRoutes;

impl RouteSystem for WindowsRoutes {
    fn default_routes(&mut self) -> io::Result<Vec<DefaultRoute>> {
        Ok(forward_rows()?
            .iter()
            .filter(|row| row.DestinationPrefix.PrefixLength == 0)
            .filter_map(|row| {
                let family = family_of(&row.DestinationPrefix.Prefix)?;
                Some(DefaultRoute {
                    family,
                    interface: alias(&row.InterfaceLuid).unwrap_or_default(),
                    interface_index: row.InterfaceIndex,
                    gateway: address_of(&row.NextHop).filter(|address| !address.is_unspecified()),
                    metric: row
                        .Metric
                        .saturating_add(interface_metric(family, row.InterfaceLuid)),
                })
            })
            .collect())
    }

    fn on_link_networks(&mut self) -> io::Result<Vec<OnLinkNetwork>> {
        let loopback = loopback_index().ok();
        Ok(forward_rows()?
            .iter()
            .filter(|row| {
                row.DestinationPrefix.PrefixLength > 0
                    && !is_ket_route(row)
                    && Some(row.InterfaceIndex) != loopback
                    && address_of(&row.NextHop).is_some_and(|hop| hop.is_unspecified())
            })
            .filter_map(|row| {
                Some(OnLinkNetwork {
                    prefix: IpPrefix::new(
                        address_of(&row.DestinationPrefix.Prefix)?,
                        row.DestinationPrefix.PrefixLength,
                    )?,
                    interface: alias(&row.InterfaceLuid).unwrap_or_default(),
                })
            })
            .collect())
    }

    fn apply(&mut self, action: &RouteAction) -> io::Result<()> {
        match action {
            RouteAction::Install {
                destination,
                path,
                replacing,
            } => {
                if let Some(previous) = replacing {
                    delete(&row_for(*destination, previous)?)?;
                }
                check(unsafe { CreateIpForwardEntry2(&row_for(*destination, path)?) })
            }
            RouteAction::Remove { destination, path } => delete(&row_for(*destination, path)?),
        }
    }

    fn clear_stale(&mut self) -> io::Result<()> {
        for row in forward_rows()?.iter().filter(|row| is_ket_route(row)) {
            delete(row)?;
        }
        Ok(())
    }
}

/// A copy of the routing table.
fn forward_rows() -> io::Result<Vec<MIB_IPFORWARD_ROW2>> {
    let mut table: *mut MIB_IPFORWARD_TABLE2 = ptr::null_mut();
    check(unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) })?;
    let rows = unsafe {
        slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize).to_vec()
    };
    unsafe { FreeMibTable(table.cast()) };
    Ok(rows)
}

/// A route Ket installed: its metric and protocol mark it.
fn is_ket_route(row: &MIB_IPFORWARD_ROW2) -> bool {
    row.Metric == ROUTE_METRIC && row.Protocol == MIB_IPPROTO_NETMGMT
}

fn row_for(destination: IpPrefix, path: &ServerPath) -> io::Result<MIB_IPFORWARD_ROW2> {
    let mut row = MIB_IPFORWARD_ROW2::default();
    unsafe { InitializeIpForwardEntry(&mut row) };
    row.DestinationPrefix.Prefix = socket_address(destination.address());
    row.DestinationPrefix.PrefixLength = destination.length();
    row.Metric = ROUTE_METRIC;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    let unspecified = if destination.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };
    match path {
        ServerPath::Via(route) => {
            row.InterfaceIndex = route.interface_index;
            row.NextHop = socket_address(route.gateway.unwrap_or(unspecified));
        }
        // Sent to the loopback interface, where it goes nowhere, instead of
        // into the tunnel the engine itself feeds.
        ServerPath::Unreachable => {
            row.InterfaceIndex = loopback_index()?;
            row.NextHop = socket_address(unspecified);
        }
    }
    Ok(row)
}

fn delete(row: &MIB_IPFORWARD_ROW2) -> io::Result<()> {
    match unsafe { DeleteIpForwardEntry2(row) } {
        ERROR_SUCCESS | ERROR_NOT_FOUND => Ok(()),
        status => Err(io::Error::from_raw_os_error(status as i32)),
    }
}

fn loopback_index() -> io::Result<u32> {
    let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
    check(unsafe { GetIfTable2(&mut table) })?;
    let index = unsafe {
        slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
            .iter()
            .find(|row| row.Type == IF_TYPE_SOFTWARE_LOOPBACK)
            .map(|row| row.InterfaceIndex)
    };
    unsafe { FreeMibTable(table.cast()) };
    index.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no loopback interface"))
}

/// What kind of network an interface is, from its interface type.
pub(crate) fn interface_kind(interface_index: u32) -> NetworkKind {
    let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
    if check(unsafe { GetIfTable2(&mut table) }).is_err() {
        return NetworkKind::Other;
    }
    let kind = unsafe {
        slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
            .iter()
            .find(|row| row.InterfaceIndex == interface_index)
            .map(|row| match row.Type {
                IF_TYPE_IEEE80211 => NetworkKind::Wifi,
                IF_TYPE_ETHERNET_CSMACD => NetworkKind::Ethernet,
                IF_TYPE_WWANPP | IF_TYPE_WWANPP2 => NetworkKind::Cellular,
                _ => NetworkKind::Other,
            })
    };
    unsafe { FreeMibTable(table.cast()) };
    kind.unwrap_or(NetworkKind::Other)
}

fn interface_metric(family: Family, luid: NET_LUID_LH) -> u32 {
    let mut row = MIB_IPINTERFACE_ROW::default();
    unsafe { InitializeIpInterfaceEntry(&mut row) };
    row.Family = match family {
        Family::V4 => AF_INET,
        Family::V6 => AF_INET6,
    };
    row.InterfaceLuid = luid;
    if unsafe { GetIpInterfaceEntry(&mut row) } == ERROR_SUCCESS {
        row.Metric
    } else {
        u32::MAX / 2
    }
}

fn alias(luid: &NET_LUID_LH) -> Option<String> {
    let mut buffer = [0_u16; IF_MAX_STRING_SIZE as usize + 1];
    (unsafe { ConvertInterfaceLuidToAlias(luid, buffer.as_mut_ptr(), buffer.len()) }
        == ERROR_SUCCESS)
        .then(|| {
            let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
            String::from_utf16_lossy(&buffer[..length])
        })
}

fn family_of(address: &SOCKADDR_INET) -> Option<Family> {
    match unsafe { address.si_family } {
        AF_INET => Some(Family::V4),
        AF_INET6 => Some(Family::V6),
        _ => None,
    }
}

fn address_of(address: &SOCKADDR_INET) -> Option<IpAddr> {
    match family_of(address)? {
        Family::V4 => Some(IpAddr::V4(Ipv4Addr::from(
            unsafe { address.Ipv4.sin_addr.S_un.S_addr }.to_ne_bytes(),
        ))),
        Family::V6 => Some(IpAddr::V6(Ipv6Addr::from(unsafe {
            address.Ipv6.sin6_addr.u.Byte
        }))),
    }
}

fn socket_address(address: IpAddr) -> SOCKADDR_INET {
    match address {
        IpAddr::V4(address) => SOCKADDR_INET {
            Ipv4: SOCKADDR_IN {
                sin_family: AF_INET,
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 {
                        S_addr: u32::from_ne_bytes(address.octets()),
                    },
                },
                ..Default::default()
            },
        },
        IpAddr::V6(address) => SOCKADDR_INET {
            Ipv6: SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                sin6_addr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: address.octets(),
                    },
                },
                ..Default::default()
            },
        },
    }
}

fn check(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}
