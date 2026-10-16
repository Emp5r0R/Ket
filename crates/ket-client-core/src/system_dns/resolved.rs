//! systemd-resolved: makes a tunnel link the resolver for every domain (`~.`),
//! so lookups through the resolved API or its 127.0.0.53 stub go into the
//! tunnel and are not sent out of the other links. resolved forgets the
//! setting when the link disappears; dropping the handle also reverts it.
//!
//! This replaces rewriting `/etc/resolv.conf` where resolved owns that file:
//! the sandboxed broker may not write it (it belongs to `systemd-resolve`),
//! and resolved rewrites it whenever the network's DNS servers change.

use std::{
    io,
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
};

/// resolved's Varlink socket; it accepts connections only while resolved runs
/// (the file outlives a stopped service).
const RESOLVED_SOCKET: &str = "/run/systemd/resolve/io.systemd.Resolve";
/// Where resolved keeps the resolver files it owns.
const RESOLVED_DIRECTORY: &str = "/run/systemd/resolve";

pub(super) fn resolved_running() -> bool {
    UnixStream::connect(RESOLVED_SOCKET).is_ok()
}

/// True when `resolver` (normally `/etc/resolv.conf`) is one of resolved's
/// own files, so resolved, not the file, decides where lookups go.
pub(super) fn resolved_owns(resolver: &Path) -> bool {
    resolver
        .canonicalize()
        .is_ok_and(|target| target.starts_with(RESOLVED_DIRECTORY))
}

#[derive(Debug)]
pub(super) struct ResolvedLink {
    interface: String,
}

impl ResolvedLink {
    /// Routes every domain to `resolvers` through `interface`. Fails with
    /// `NotFound` while the interface does not exist yet.
    pub(super) fn route_all_lookups(interface: &str, resolvers: &[&str]) -> io::Result<Self> {
        if !Path::new("/sys/class/net").join(interface).exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the tunnel interface {interface} does not exist yet"),
            ));
        }
        let link = Self {
            interface: interface.to_owned(),
        };
        for arguments in configuration(interface, resolvers) {
            resolvectl(&arguments)?;
        }
        Ok(link)
    }
}

impl Drop for ResolvedLink {
    fn drop(&mut self) {
        let _ = resolvectl(&["revert", &self.interface]);
    }
}

fn configuration<'a>(interface: &'a str, resolvers: &[&'a str]) -> [Vec<&'a str>; 3] {
    let mut dns = vec!["dns", interface];
    dns.extend_from_slice(resolvers);
    [
        dns,
        vec!["domain", interface, "~."],
        vec!["default-route", interface, "yes"],
    ]
}

fn resolvectl(arguments: &[&str]) -> io::Result<()> {
    let status = Command::new("resolvectl")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "resolvectl {} failed",
            arguments.join(" ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tunnel_link_resolves_every_domain_through_the_tunnel_resolvers() {
        assert_eq!(
            configuration("ket0", &["198.18.0.1"]),
            [
                vec!["dns", "ket0", "198.18.0.1"],
                vec!["domain", "ket0", "~."],
                vec!["default-route", "ket0", "yes"],
            ]
        );
        assert_eq!(
            configuration("ketvpn0", &["1.1.1.1", "1.0.0.1"])[0],
            vec!["dns", "ketvpn0", "1.1.1.1", "1.0.0.1"]
        );
    }

    #[test]
    fn a_missing_interface_is_reported_as_not_yet_created() {
        let error = ResolvedLink::route_all_lookups("ket-test-missing0", &["198.18.0.1"])
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn only_files_under_resolveds_directory_belong_to_it() {
        let directory = std::env::temp_dir().join(format!("ket-resolved-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("resolv.conf");
        std::fs::write(&file, b"nameserver 192.0.2.53\n").unwrap();
        assert!(!resolved_owns(&file));
        assert!(!resolved_owns(&directory.join("missing")));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
