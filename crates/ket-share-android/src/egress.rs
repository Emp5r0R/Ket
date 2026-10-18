//! The phone's way out: every connection and lookup is tied to Ket's own VPN
//! network with the NDK's `android_setsocknetwork` and
//! `android_getaddrinfofornetwork` (API 23). The app sets the network handle
//! whenever the VPN comes up or goes down; with none, nothing is dialled.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use ket_share::{Egress, EgressError, Host, Ipv4Range};
use tokio::net::{TcpSocket, TcpStream};

/// hev-socks5-tunnel's mapped DNS answers names from this range (see
/// `AndroidVpnDnsPolicy`).
pub const STAND_INS: Ipv4Range = Ipv4Range::new(Ipv4Addr::new(240, 0, 0, 0), 4);

/// `Network.getNetworkHandle()` of the VPN, or 0 while there is none.
#[derive(Clone, Default)]
pub struct VpnNetwork(Arc<AtomicU64>);

impl VpnNetwork {
    pub fn set(&self, handle: u64) {
        self.0.store(handle, Ordering::SeqCst);
    }

    fn current(&self) -> Result<u64, EgressError> {
        match self.0.load(Ordering::SeqCst) {
            0 => Err(EgressError::NoTunnel),
            handle => Ok(handle),
        }
    }
}

pub struct VpnEgress {
    pub network: VpnNetwork,
}

#[async_trait]
impl Egress for VpnEgress {
    async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, EgressError> {
        let network = self.network.current()?;
        // Names reach here already checked (`Host::parse`), so they hold no
        // NUL byte.
        debug_assert!(matches!(Host::parse(name), Ok(Host::Name(_))));
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || platform::resolve(network, &name))
            .await
            .map_err(|_| EgressError::Unresolved)?
    }

    async fn connect(&self, address: SocketAddr) -> Result<TcpStream, EgressError> {
        let network = self.network.current()?;
        let socket = match address {
            SocketAddr::V4(_) => TcpSocket::new_v4(),
            SocketAddr::V6(_) => TcpSocket::new_v6(),
        }
        .map_err(|_| EgressError::Unreachable)?;
        platform::bind(network, &socket).map_err(|_| EgressError::NoTunnel)?;
        socket
            .connect(address)
            .await
            .map_err(|error| EgressError::from_connect(&error))
    }

    fn synthetic_range(&self) -> Option<Ipv4Range> {
        Some(STAND_INS)
    }
}

#[cfg(target_os = "android")]
mod platform {
    use std::{
        ffi::CString,
        io,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
        os::fd::AsRawFd,
        ptr,
    };

    use ket_share::EgressError;
    use tokio::net::TcpSocket;

    #[link(name = "android")]
    unsafe extern "C" {
        fn android_setsocknetwork(network: u64, fd: libc::c_int) -> libc::c_int;
        fn android_getaddrinfofornetwork(
            network: u64,
            node: *const libc::c_char,
            service: *const libc::c_char,
            hints: *const libc::addrinfo,
            result: *mut *mut libc::addrinfo,
        ) -> libc::c_int;
    }

    pub(super) fn bind(network: u64, socket: &TcpSocket) -> io::Result<()> {
        if unsafe { android_setsocknetwork(network, socket.as_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn resolve(network: u64, name: &str) -> Result<Vec<IpAddr>, EgressError> {
        let node = CString::new(name).map_err(|_| EgressError::Unresolved)?;
        let hints = libc::addrinfo {
            ai_flags: 0,
            ai_family: libc::AF_UNSPEC,
            ai_socktype: libc::SOCK_STREAM,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_canonname: ptr::null_mut(),
            ai_addr: ptr::null_mut(),
            ai_next: ptr::null_mut(),
        };
        let mut list: *mut libc::addrinfo = ptr::null_mut();
        let status = unsafe {
            android_getaddrinfofornetwork(network, node.as_ptr(), ptr::null(), &hints, &mut list)
        };
        if status != 0 {
            return Err(EgressError::Unresolved);
        }
        let mut addresses = Vec::new();
        let mut entry = list;
        while !entry.is_null() {
            let current = unsafe { &*entry };
            entry = current.ai_next;
            if current.ai_addr.is_null() {
                continue;
            }
            match current.ai_family {
                libc::AF_INET => {
                    let address = unsafe { &*current.ai_addr.cast::<libc::sockaddr_in>() };
                    addresses.push(IpAddr::V4(Ipv4Addr::from(u32::from_be(
                        address.sin_addr.s_addr,
                    ))));
                }
                libc::AF_INET6 => {
                    let address = unsafe { &*current.ai_addr.cast::<libc::sockaddr_in6>() };
                    addresses.push(IpAddr::V6(Ipv6Addr::from(address.sin6_addr.s6_addr)));
                }
                _ => {}
            }
        }
        unsafe { libc::freeaddrinfo(list) };
        if addresses.is_empty() {
            Err(EgressError::Unresolved)
        } else {
            Ok(addresses)
        }
    }
}

/// Off Android (the build machine's own checks) there is no VPN network to
/// tie to, so nothing is ever dialled.
#[cfg(not(target_os = "android"))]
mod platform {
    use std::{io, net::IpAddr};

    use ket_share::EgressError;
    use tokio::net::TcpSocket;

    pub(super) fn bind(_network: u64, _socket: &TcpSocket) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(super) fn resolve(_network: u64, _name: &str) -> Result<Vec<IpAddr>, EgressError> {
        Err(EgressError::NoTunnel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn without_a_vpn_network_nothing_is_dialled() {
        let egress = VpnEgress {
            network: VpnNetwork::default(),
        };
        assert!(matches!(
            egress.connect("1.1.1.1:443".parse().unwrap()).await,
            Err(EgressError::NoTunnel)
        ));
        assert!(matches!(
            egress.resolve("example.com").await,
            Err(EgressError::NoTunnel)
        ));
        assert_eq!(egress.synthetic_range(), Some(STAND_INS));
    }

    #[tokio::test]
    async fn a_network_that_cannot_be_bound_is_no_tunnel() {
        let network = VpnNetwork::default();
        network.set(42);
        let egress = VpnEgress { network };
        // Off Android binding always fails, as it does on a phone whose VPN
        // network has just gone.
        assert!(matches!(
            egress.connect("1.1.1.1:443".parse().unwrap()).await,
            Err(EgressError::NoTunnel)
        ));
    }
}
