//! Local addresses through `getifaddrs`, the tunnel from `/sys/class/net`,
//! and connections tied to it with `SO_BINDTODEVICE`, which an unprivileged
//! process may set on a new socket since Linux 5.7.

use std::{ffi::CStr, fs, io, net::Ipv4Addr, path::Path};

use tokio::net::{TcpSocket, UdpSocket};

use super::plan::{self, Binding, LocalAddress, Tunnel};
use crate::route_keeper::linux::interface_kind;

const IFF_UP: u32 = 0x1;

pub(crate) fn local_addresses() -> io::Result<Vec<LocalAddress>> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut addresses = Vec::new();
    let mut entry = list;
    while !entry.is_null() {
        let current = unsafe { &*entry };
        entry = current.ifa_next;
        let flags = current.ifa_flags;
        if current.ifa_addr.is_null()
            || flags & libc::IFF_UP as u32 == 0
            || flags & libc::IFF_RUNNING as u32 == 0
            || flags & libc::IFF_LOOPBACK as u32 != 0
            || i32::from(unsafe { (*current.ifa_addr).sa_family }) != libc::AF_INET
        {
            continue;
        }
        let address = unsafe { &*current.ifa_addr.cast::<libc::sockaddr_in>() };
        let address = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
        let interface = unsafe { CStr::from_ptr(current.ifa_name) }
            .to_string_lossy()
            .into_owned();
        addresses.push(LocalAddress {
            kind: interface_kind(&interface),
            tunnel: interface == plan::LINUX_BRIDGE || interface == plan::LINUX_OPENVPN,
            interface,
            address,
        });
    }
    unsafe { libc::freeifaddrs(list) };
    Ok(addresses)
}

pub(crate) fn tunnel() -> Option<Tunnel> {
    plan::linux_tunnel(is_up)
}

fn is_up(interface: &str) -> bool {
    let flags = Path::new("/sys/class/net").join(interface).join("flags");
    fs::read_to_string(flags)
        .ok()
        .and_then(|text| u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok())
        .is_some_and(|flags| flags & IFF_UP != 0)
}

pub(crate) fn bind_tcp(socket: &TcpSocket, binding: &Binding, _ipv6: bool) -> io::Result<()> {
    socket.bind_device(Some(device(binding)?))
}

pub(crate) fn bind_udp(socket: &UdpSocket, binding: &Binding) -> io::Result<()> {
    socket.bind_device(Some(device(binding)?))
}

fn device(binding: &Binding) -> io::Result<&'static [u8]> {
    match binding {
        Binding::Device(name) => Ok(name.as_bytes()),
        Binding::Index(_) => Err(io::Error::from(io::ErrorKind::Unsupported)),
    }
}
