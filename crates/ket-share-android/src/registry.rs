//! The shares the app has running, by handle. Each has its own small
//! runtime, so starting and stopping one never depends on another, and the
//! JNI calls stay plain blocking functions.

use std::{
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddrV4},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use ket_share::{Credentials, ShareServer, ShareStats};
use tokio::runtime::{Builder, Runtime};

use crate::egress::{VpnEgress, VpnNetwork};

/// The port tried first, the same as the desktop app's.
pub const PREFERRED_PORT: u16 = 39740;
const WORKER_THREADS: usize = 2;

struct Running {
    runtime: Runtime,
    server: ShareServer,
    network: VpnNetwork,
}

static SHARES: LazyLock<Mutex<HashMap<i64, Running>>> = LazyLock::new(Mutex::default);
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

#[derive(Debug)]
pub struct Started {
    pub handle: i64,
    pub port: u16,
    pub credentials: Credentials,
}

#[derive(Debug, Eq, PartialEq)]
pub enum StartError {
    /// Only a private (RFC 1918) address of this phone can be shared on.
    NotPrivate,
    PortUnavailable,
}

impl StartError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotPrivate => "network_unavailable",
            Self::PortUnavailable => "port_unavailable",
        }
    }
}

/// Starts sharing on `address` with a new password. Its connections go
/// nowhere until [`set_network`] names the VPN network.
pub fn start(address: Ipv4Addr) -> Result<Started, StartError> {
    if !address.is_private() {
        return Err(StartError::NotPrivate);
    }
    let runtime = Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .thread_name("ket-share")
        .enable_all()
        .build()
        .map_err(|_| StartError::PortUnavailable)?;
    let network = VpnNetwork::default();
    let credentials = Credentials::generate();
    let egress = Arc::new(VpnEgress {
        network: network.clone(),
    });
    let server = runtime
        .block_on(async {
            match ShareServer::start(
                SocketAddrV4::new(address, PREFERRED_PORT),
                credentials.clone(),
                egress.clone(),
            )
            .await
            {
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                    ShareServer::start(SocketAddrV4::new(address, 0), credentials.clone(), egress)
                        .await
                }
                result => result,
            }
        })
        .map_err(|_| StartError::PortUnavailable)?;
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let port = server.address().port();
    SHARES.lock().expect("shares").insert(
        handle,
        Running {
            runtime,
            server,
            network,
        },
    );
    Ok(Started {
        handle,
        port,
        credentials,
    })
}

/// The VPN network's handle, or 0 while the VPN is down.
pub fn set_network(handle: i64, network: u64) -> bool {
    match SHARES.lock().expect("shares").get(&handle) {
        Some(running) => {
            running.network.set(network);
            true
        }
        None => false,
    }
}

pub fn stats(handle: i64) -> Option<ShareStats> {
    SHARES
        .lock()
        .expect("shares")
        .get(&handle)
        .map(|running| running.server.stats())
}

/// Stops the share and closes its connections. Unknown handles are ignored.
pub fn stop(handle: i64) {
    let Some(running) = SHARES.lock().expect("shares").remove(&handle) else {
        return;
    };
    running.network.set(0);
    running.server.stop();
    running.runtime.block_on(running.server.closed());
    drop(running.server);
    running
        .runtime
        .shutdown_timeout(std::time::Duration::from_secs(1));
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;

    use super::*;

    #[test]
    fn only_private_addresses_of_this_device_are_shared() {
        let started = start(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(
            started.err(),
            Some(StartError::NotPrivate),
            "loopback is not a network"
        );

        // Private addresses this machine does not have cannot be listened on.
        let missing = start(Ipv4Addr::new(10, 255, 255, 254));
        assert_eq!(missing.err(), Some(StartError::PortUnavailable));
    }

    #[test]
    fn unknown_handles_are_harmless() {
        assert!(!set_network(-1, 7));
        assert_eq!(stats(-1), None);
        stop(-1);
    }

    /// A share on this machine's own private address, when it has one.
    #[test]
    fn a_share_on_a_local_address_runs_until_stopped() {
        let Some(address) = local_private_address() else {
            return;
        };
        let started = start(address).expect("share starts");
        assert_eq!(started.credentials.username(), "ket");
        assert!(set_network(started.handle, 0));
        assert_eq!(stats(started.handle), Some(ShareStats::default()));
        assert!(TcpStream::connect((address, started.port)).is_ok());
        stop(started.handle);
        assert_eq!(stats(started.handle), None);
        assert!(TcpStream::connect((address, started.port)).is_err());
    }

    fn local_private_address() -> Option<Ipv4Addr> {
        // The address the machine would use toward a private network; no
        // packet is sent by connecting a UDP socket.
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("10.255.255.255:9").ok()?;
        match socket.local_addr().ok()?.ip() {
            std::net::IpAddr::V4(address) if address.is_private() => Some(address),
            _ => None,
        }
    }
}
