//! Shares a device's Ket connection with nearby devices: a SOCKS5 and HTTP
//! proxy on one port of one local address, signed in with a password drawn
//! when sharing starts, that reaches only the public Internet and only
//! through the tunnel.
//!
//! The platform decides where to listen (an address on the local network or
//! hotspot, never every address) and supplies the [`Egress`] that ties each
//! connection to the tunnel. Because the share sits in front of the tunnel
//! rather than inside an engine, it keeps its port when Ket switches
//! protocol, and works the same with every one.

mod credentials;
pub mod dns;
mod egress;
mod handshake;
mod http;
mod limiter;
mod policy;
mod server;
mod socks5;
mod target;

pub use credentials::{Credentials, PASSWORD_LENGTH, USERNAME};
pub use egress::{Egress, EgressError};
pub use limiter::{MAX_FAILURES, MAX_TOTAL_FAILURES, WINDOW};
pub use policy::{Ipv4Range, Origin, allowed};
pub use server::{MAX_CONNECTIONS, ShareServer, ShareStats};
pub use target::{Host, Target, TargetError};
