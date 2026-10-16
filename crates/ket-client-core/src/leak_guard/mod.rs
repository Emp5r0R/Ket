//! Keeps name lookups and IPv6 inside a full-route tunnel on Windows for as
//! long as it is up.
//!
//! The routes alone do not: Windows asks every adapter's resolvers in
//! parallel (Smart Multi-Homed Name Resolution) and reaches link-local IPv6
//! resolvers directly, and the bridge routes only IPv4 there. The guard is
//! engaged before the bridge starts, admits the tunnel interface once it
//! exists, and is released by dropping it. Filtering Platform filters
//! ([`plan`]) do the blocking. Elsewhere the guard does nothing; on Linux the
//! system DNS lease routes lookups into the tunnel instead.

#[cfg(any(windows, test))]
mod plan;
#[cfg(windows)]
mod windows;

use std::{io, net::IpAddr};

pub(crate) struct LeakGuard {
    #[cfg(windows)]
    platform: windows::PlatformGuard,
    admitted: bool,
}

impl LeakGuard {
    /// Blocks lookups outside the tunnel before the tunnel exists; `servers`
    /// are the addresses the engines must still reach directly.
    pub(crate) fn engage(servers: &[IpAddr]) -> io::Result<Self> {
        #[cfg(not(windows))]
        let _ = servers;
        Ok(Self {
            #[cfg(windows)]
            platform: windows::PlatformGuard::engage(servers)?,
            admitted: false,
        })
    }

    /// Lets the tunnel interface carry lookups. Fails with `NotFound` while
    /// the bridge has not created it yet.
    pub(crate) fn admit_tunnel(&mut self, interface: &str) -> io::Result<()> {
        if self.admitted {
            return Ok(());
        }
        #[cfg(windows)]
        self.platform.admit_tunnel(interface)?;
        #[cfg(not(windows))]
        let _ = interface;
        self.admitted = true;
        Ok(())
    }
}
