//! Local addresses through the IP Helper API, and connections tied to the
//! tunnel with `IP_UNICAST_IF`, which needs no privileges.
//!
//! The tunnel is the bridge's adapter (named `Ket`) or, for OpenVPN, which
//! names its own adapter, the interface carrying OpenVPN's `0.0.0.0/1`
//! redirect route.

use std::{io, net::Ipv4Addr, os::windows::io::AsRawSocket, ptr, slice};

use tokio::net::{TcpSocket, UdpSocket};
use windows_sys::Win32::{
    NetworkManagement::{
        IpHelper::{
            ConvertInterfaceAliasToLuid, ConvertInterfaceLuidToIndex, FreeMibTable, GetIfTable2,
            GetUnicastIpAddressTable, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2,
            MIB_UNICASTIPADDRESS_TABLE,
        },
        Ndis::{IfOperStatusUp, NET_LUID_LH},
    },
    Networking::WinSock::{
        AF_INET, IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, setsockopt,
    },
};

use super::plan::{BRIDGE_RESOLVERS, Binding, LocalAddress, OPENVPN_RESOLVERS, Tunnel};
use crate::{
    full_route::TUNNEL_INTERFACE,
    route_keeper::windows::{address_of, alias, check, forward_rows, interface_kind},
};

pub(crate) fn local_addresses() -> io::Result<Vec<LocalAddress>> {
    let up = interfaces_up()?;
    let openvpn = openvpn_index();
    let mut table: *mut MIB_UNICASTIPADDRESS_TABLE = ptr::null_mut();
    check(unsafe { GetUnicastIpAddressTable(AF_INET, &mut table) })?;
    let rows = unsafe {
        slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize).to_vec()
    };
    unsafe { FreeMibTable(table.cast()) };
    Ok(rows
        .iter()
        .filter(|row| up.contains(&row.InterfaceIndex))
        .filter_map(|row| {
            let std::net::IpAddr::V4(address) = address_of(&row.Address)? else {
                return None;
            };
            let interface = alias(&row.InterfaceLuid).unwrap_or_default();
            Some(LocalAddress {
                tunnel: interface == TUNNEL_INTERFACE || Some(row.InterfaceIndex) == openvpn,
                kind: interface_kind(row.InterfaceIndex),
                interface,
                address,
            })
        })
        .collect())
}

/// Interfaces that are up, loopback left out.
fn interfaces_up() -> io::Result<Vec<u32>> {
    let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
    check(unsafe { GetIfTable2(&mut table) })?;
    let indexes = unsafe {
        slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
            .iter()
            .filter(|row| row.OperStatus == IfOperStatusUp && row.Type != IF_TYPE_SOFTWARE_LOOPBACK)
            .map(|row| row.InterfaceIndex)
            .collect()
    };
    unsafe { FreeMibTable(table.cast()) };
    Ok(indexes)
}

pub(crate) fn tunnel() -> Option<Tunnel> {
    if let Some(index) = bridge_index() {
        return Some(Tunnel {
            binding: Binding::Index(index),
            resolvers: BRIDGE_RESOLVERS,
        });
    }
    openvpn_index().map(|index| Tunnel {
        binding: Binding::Index(index),
        resolvers: OPENVPN_RESOLVERS,
    })
}

fn bridge_index() -> Option<u32> {
    let mut luid = NET_LUID_LH::default();
    let name = TUNNEL_INTERFACE
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    check(unsafe { ConvertInterfaceAliasToLuid(name.as_ptr(), &mut luid) }).ok()?;
    let mut index = 0;
    check(unsafe { ConvertInterfaceLuidToIndex(&luid, &mut index) }).ok()?;
    interfaces_up().ok()?.contains(&index).then_some(index)
}

/// OpenVPN's `redirect-gateway def1` sends 0.0.0.0/1 into its adapter.
fn openvpn_index() -> Option<u32> {
    forward_rows()
        .ok()?
        .iter()
        .find(|row| {
            row.DestinationPrefix.PrefixLength == 1
                && address_of(&row.DestinationPrefix.Prefix)
                    == Some(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        })
        .map(|row| row.InterfaceIndex)
}

pub(crate) fn bind_tcp(socket: &TcpSocket, binding: &Binding, ipv6: bool) -> io::Result<()> {
    bind(socket.as_raw_socket(), binding, ipv6)
}

pub(crate) fn bind_udp(socket: &UdpSocket, binding: &Binding) -> io::Result<()> {
    bind(socket.as_raw_socket(), binding, false)
}

fn bind(socket: u64, binding: &Binding, ipv6: bool) -> io::Result<()> {
    let Binding::Index(index) = binding else {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    };
    // IPv4 takes the index in network byte order, IPv6 in host order.
    let (level, option, value) = if ipv6 {
        (IPPROTO_IPV6, IPV6_UNICAST_IF, *index)
    } else {
        (IPPROTO_IP, IP_UNICAST_IF, index.to_be())
    };
    let status = unsafe {
        setsockopt(
            socket as usize,
            level,
            option,
            (&value as *const u32).cast(),
            size_of::<u32>() as i32,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
