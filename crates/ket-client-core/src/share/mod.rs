//! Sharing this computer's Ket connection with nearby devices (desktop).
//!
//! The proxy itself is `ket-share`; this module supplies what is specific to
//! the desktop: the local networks the share can listen on
//! ([`share_networks`]), the way out through whichever tunnel is up
//! ([`TunnelEgress`]), and a [`ShareSession`] that ends on its own when the
//! network it listens on goes away. The session runs in the unprivileged app,
//! never in the tunnel service, so input from nearby devices is never read
//! by a privileged process.

#[cfg(target_os = "linux")]
mod linux;
mod plan;
#[cfg(windows)]
mod windows;

use std::{
    fmt, io,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use ket_share::{Credentials, Egress, EgressError, Ipv4Range, ShareServer};
use serde::Serialize;
use tokio::{
    net::{TcpSocket, TcpStream, UdpSocket},
    task::JoinHandle,
};

#[cfg(target_os = "linux")]
use linux as platform;
pub use plan::ShareNetwork;
#[cfg(windows)]
use windows as platform;

/// The port the share tries first, so a device set up once keeps working
/// the next time; a free one is picked when it is taken.
pub const PREFERRED_PORT: u16 = 39740;
/// How often the session checks that its network is still there.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// The bridge's stand-in addresses for names (`tun2proxy --dns virtual`).
const BRIDGE_STAND_INS: Ipv4Range = Ipv4Range::new(Ipv4Addr::new(198, 18, 0, 0), 15);
const DNS_PORT: u16 = 53;

/// The networks this computer can share on now, best first.
pub fn share_networks() -> Vec<ShareNetwork> {
    plan::share_networks(local_addresses())
}

fn local_addresses() -> Vec<plan::LocalAddress> {
    #[cfg(any(target_os = "linux", windows))]
    {
        platform::local_addresses().unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

fn current_tunnel() -> Option<plan::Tunnel> {
    #[cfg(any(target_os = "linux", windows))]
    {
        platform::tunnel()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// Connections and lookups tied to whichever tunnel is up when they start.
/// With no tunnel they fail, and one that goes down takes its connections
/// with it: nothing a nearby device sends can leave through the local
/// network.
#[derive(Clone, Copy, Debug, Default)]
pub struct TunnelEgress;

impl TunnelEgress {
    pub fn tunnel_is_up(&self) -> bool {
        current_tunnel().is_some()
    }
}

#[async_trait]
impl Egress for TunnelEgress {
    async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, EgressError> {
        let tunnel = current_tunnel().ok_or(EgressError::NoTunnel)?;
        let mut failure = EgressError::TimedOut;
        for resolver in tunnel.resolvers {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                .await
                .map_err(|_| EgressError::NoTunnel)?;
            bind_udp(&socket, &tunnel.binding).map_err(|_| EgressError::NoTunnel)?;
            socket
                .connect((*resolver, DNS_PORT))
                .await
                .map_err(|_| EgressError::NoTunnel)?;
            match ket_share::dns::lookup(&socket, name).await {
                Err(EgressError::TimedOut) => failure = EgressError::TimedOut,
                result => return result,
            }
        }
        Err(failure)
    }

    async fn connect(&self, address: SocketAddr) -> Result<TcpStream, EgressError> {
        let tunnel = current_tunnel().ok_or(EgressError::NoTunnel)?;
        let socket = match address {
            SocketAddr::V4(_) => TcpSocket::new_v4(),
            SocketAddr::V6(_) => TcpSocket::new_v6(),
        }
        .map_err(|_| EgressError::Unreachable)?;
        bind_tcp(&socket, &tunnel.binding, address.is_ipv6()).map_err(|_| EgressError::NoTunnel)?;
        socket.connect(address).await.map_err(|error| {
            if is_missing_device(&error) {
                EgressError::NoTunnel
            } else {
                EgressError::from_connect(&error)
            }
        })
    }

    fn synthetic_range(&self) -> Option<Ipv4Range> {
        Some(BRIDGE_STAND_INS)
    }
}

#[cfg(any(target_os = "linux", windows))]
fn bind_tcp(socket: &TcpSocket, binding: &plan::Binding, ipv6: bool) -> io::Result<()> {
    platform::bind_tcp(socket, binding, ipv6)
}

#[cfg(any(target_os = "linux", windows))]
fn bind_udp(socket: &UdpSocket, binding: &plan::Binding) -> io::Result<()> {
    platform::bind_udp(socket, binding)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn bind_tcp(_socket: &TcpSocket, _binding: &plan::Binding, _ipv6: bool) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn bind_udp(_socket: &UdpSocket, _binding: &plan::Binding) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// The tunnel interface vanished between the check and the connection.
fn is_missing_device(error: &io::Error) -> bool {
    #[cfg(target_os = "linux")]
    {
        error.raw_os_error() == Some(libc::ENODEV)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = error;
        false
    }
}

/// What a nearby device needs to use the share.
#[derive(Clone, Serialize)]
pub struct ShareDetails {
    pub network: ShareNetwork,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for ShareDetails {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShareDetails")
            .field("network", &self.network)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ShareStatus {
    pub devices: usize,
    pub refused_sign_ins: u64,
    /// Whether a tunnel is up to carry the devices' traffic right now; while
    /// Ket reconnects or switches protocol it is not, and devices are told so.
    pub tunnel: bool,
    /// Set once the share has stopped on its own.
    pub ended: Option<ShareEnd>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareEnd {
    /// The network the share listened on went away.
    NetworkGone,
}

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("this computer is not on that network now")]
    NetworkUnavailable,
    #[error("could not open a port for sharing")]
    PortUnavailable(#[source] io::Error),
}

impl ShareError {
    /// A stable name for the app to show its own wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NetworkUnavailable => "network_unavailable",
            Self::PortUnavailable(_) => "port_unavailable",
        }
    }
}

pub struct ShareSession {
    server: Arc<ShareServer>,
    details: ShareDetails,
    network_gone: Arc<AtomicBool>,
    watcher: JoinHandle<()>,
}

impl ShareSession {
    /// Starts sharing on `address`, which must be one of
    /// [`share_networks`], with a new password.
    pub async fn start(address: Ipv4Addr) -> Result<Self, ShareError> {
        Self::start_with(address, Arc::new(TunnelEgress)).await
    }

    async fn start_with(address: Ipv4Addr, egress: Arc<dyn Egress>) -> Result<Self, ShareError> {
        let network = share_networks()
            .into_iter()
            .find(|network| network.address == address)
            .ok_or(ShareError::NetworkUnavailable)?;
        let credentials = Credentials::generate();
        let server = match ShareServer::start(
            SocketAddrV4::new(address, PREFERRED_PORT),
            credentials.clone(),
            egress.clone(),
        )
        .await
        {
            Ok(server) => server,
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                ShareServer::start(SocketAddrV4::new(address, 0), credentials.clone(), egress)
                    .await
                    .map_err(ShareError::PortUnavailable)?
            }
            Err(error) => return Err(ShareError::PortUnavailable(error)),
        };
        let server = Arc::new(server);
        let network_gone = Arc::new(AtomicBool::new(false));
        let watcher = tokio::spawn(watch_network(address, server.clone(), network_gone.clone()));
        Ok(Self {
            details: ShareDetails {
                network,
                port: server.address().port(),
                username: credentials.username().to_owned(),
                password: credentials.password().to_owned(),
            },
            server,
            network_gone,
            watcher,
        })
    }

    pub fn details(&self) -> &ShareDetails {
        &self.details
    }

    pub fn status(&self) -> ShareStatus {
        let stats = self.server.stats();
        ShareStatus {
            devices: stats.devices,
            refused_sign_ins: stats.refused_sign_ins,
            tunnel: current_tunnel().is_some(),
            ended: self
                .network_gone
                .load(Ordering::Relaxed)
                .then_some(ShareEnd::NetworkGone),
        }
    }

    /// Stops sharing and closes every device's connections.
    pub async fn stop(self) {
        self.watcher.abort();
        self.server.stop();
        self.server.closed().await;
    }
}

async fn watch_network(address: Ipv4Addr, server: Arc<ShareServer>, network_gone: Arc<AtomicBool>) {
    loop {
        tokio::time::sleep(WATCH_INTERVAL).await;
        if server.is_stopped() {
            return;
        }
        if !local_addresses()
            .iter()
            .any(|local| local.address == address)
        {
            network_gone.store(true, Ordering::Relaxed);
            server.stop();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sharing_needs_an_address_of_this_computer() {
        let error = ShareSession::start(Ipv4Addr::new(192, 0, 2, 1))
            .await
            .err()
            .expect("not a local address");
        assert_eq!(error.code(), "network_unavailable");
    }

    #[test]
    fn shared_details_never_print_the_password() {
        let details = ShareDetails {
            network: ShareNetwork {
                interface: "wlan0".to_owned(),
                address: Ipv4Addr::new(192, 168, 1, 5),
                kind: crate::NetworkKind::Wifi,
            },
            port: PREFERRED_PORT,
            username: "ket".to_owned(),
            password: "abcdefghjkmn".to_owned(),
        };
        assert!(!format!("{details:?}").contains("abcdefghjkmn"));
    }

    /// The tests never run with a Ket tunnel up, so the egress must refuse.
    #[tokio::test]
    async fn without_a_tunnel_the_egress_refuses_to_dial_or_look_up() {
        if TunnelEgress.tunnel_is_up() {
            return;
        }
        assert!(matches!(
            TunnelEgress.connect("1.1.1.1:443".parse().unwrap()).await,
            Err(EgressError::NoTunnel)
        ));
        assert!(matches!(
            TunnelEgress.resolve("example.com").await,
            Err(EgressError::NoTunnel)
        ));
    }
}
