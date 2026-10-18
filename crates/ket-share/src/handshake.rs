//! What can go wrong before a nearby device is let through, shared by the
//! SOCKS and HTTP front doors.

use crate::target::TargetError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// The device sent a user name and password, and they were wrong. Only
    /// this counts toward the sign-in limit.
    BadCredentials,
    /// The device has not signed in yet (a browser's first try before it
    /// asks for the password).
    NoCredentials,
    /// Not SOCKS5 or HTTP, cut short, or too long.
    Malformed,
    /// A SOCKS command other than `CONNECT`, or an HTTP request the share
    /// does not forward (such as `https://` without `CONNECT`).
    Unsupported,
    Target(TargetError),
}
