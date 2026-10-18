//! The way out: how the share reaches a destination through the tunnel.
//!
//! Each platform supplies its own. Every connection and every name lookup
//! must be tied to the tunnel itself (bound to its interface or network), so
//! when the tunnel is down they fail instead of leaving through the local
//! network.

use std::{io, net::IpAddr, net::SocketAddr};

use async_trait::async_trait;
use thiserror::Error;
use tokio::net::TcpStream;

use crate::policy::Ipv4Range;

#[derive(Debug, Error)]
pub enum EgressError {
    /// There is no tunnel right now (connecting, switching protocol, or the
    /// network is gone).
    #[error("Ket is not connected")]
    NoTunnel,
    #[error("the name could not be found")]
    Unresolved,
    #[error("the destination did not answer in time")]
    TimedOut,
    #[error("the destination refused the connection")]
    Refused,
    #[error("the destination cannot be reached")]
    Unreachable,
}

impl EgressError {
    pub fn from_connect(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::ConnectionRefused => Self::Refused,
            io::ErrorKind::TimedOut => Self::TimedOut,
            _ => Self::Unreachable,
        }
    }
}

#[async_trait]
pub trait Egress: Send + Sync + 'static {
    /// Looks a name up through the tunnel's own resolver.
    async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, EgressError>;

    /// Opens a TCP connection that can only leave through the tunnel.
    async fn connect(&self, address: SocketAddr) -> Result<TcpStream, EgressError>;

    /// The range the tunnel's resolver hands stand-in addresses out of, if it
    /// does (see [`crate::policy::allowed`]).
    fn synthetic_range(&self) -> Option<Ipv4Range>;
}
