# Shared client core

`ket-client-core` is the orchestration boundary shared by the Linux and Windows desktop shells. Android consumes the same contracts while providing `VpnService`-owned transport adapters instead of launching desktop route-management processes.

## Implemented lifecycle

1. `ControlEndpoint` accepts HTTPS and loopback HTTP by default. Embedded credentials, URL query secrets, fragments, redirects, ambient system proxies, oversized responses, and TLS older than 1.2 are rejected.
2. `KetClient::enroll` validates the 32-character access code locally, exchanges it once, then validates the complete untrusted manifest before retaining the session in memory. Tokens, lease expiry, node text and URL fields, map coordinates, health telemetry, transport counts, identifiers, endpoints, option maps, and credential sizes are all bounded; a valid lease is released if another manifest field fails validation.
3. `TransportSelector` ranks adapters using configured priority, automatic policy preference, recent latency, consecutive failures, and bounded cooldown. An explicit user protocol selection filters the candidate set to that protocol; multiple endpoints of the selected protocol may be retried, but another protocol is never substituted.
4. `KetClient::connect_with_protocol` records the current user selection, then probes and attempts at most the configured number of matching transports. `connect` retains automatic cross-protocol selection as the compatibility default. After a route completes its certificate path check and full-route bridge startup, Ket retains its resolved server addresses for the current in-memory profile. A later reconnect may reuse those addresses only when fresh DNS resolution fails and the transport ID, endpoint, port, protocol, and TLS name still match; the engine must repeat its certificate and route-startup checks before Ket publishes `Connected`. Every other failure is recorded before an eligible retry.
5. A serializable `ClientSnapshot` publishes the server-offered transports, current preference, and active transport through a Tokio watch channel for UI parity. It contains no control token, transport credential, or secret option values.
6. `refresh`, `renew`, and the optional maintenance task update node health, capacity, traffic, expiry, and reconnect state. Desktop schedules renewal at the midpoint of every issued lease instead of waiting until its final minute. One control timeout is tolerated. Starting with the second consecutive timeout, Ket checks both Google and Cloudflare HTTPS 204 endpoints through the current route. A successful check preserves the working tunnel while control refresh continues to report its bounded error; a failed check stops the stale route, restores host DNS, and rebuilds the route under the same explicit protocol or Automatic policy. Reconnect intent survives a failed recovery round only after an established route is lost. A manual connection attempt that exhausts its selected candidates stops in `Error` until the user explicitly retries, and `stop_tunnel` also clears reconnect intent. Every status response must carry the session ID encoded by the active token, the exact enrolled device name, a future lease expiry, valid node telemetry, and internally consistent traffic data before it can replace the last known-good snapshot. Malformed status is reported as `invalid_server_response` without publishing its fields. Authorization loss stops the tunnel, clears reconnect intent, and clears the in-memory session.
7. `disconnect` stops the local tunnel before releasing the server lease. A failed release is safe because the bounded server lease still expires.

### Changing networks

Moving from Wi-Fi to Ethernet, to a hotspot, or losing the network for a while does not end the connection:

- The engines reach the server through host routes that Ket owns (`route_keeper`), not the ones the bridge copies from the network it started on. Ket checks the routing table every 2 seconds and moves those routes to the new default route, so the tunnel interface stays up and only the engines' own connections are redialled. Linux reads `/proc/net` and changes routes with `ip route` (tagged `proto 75`); Windows uses the IP Helper API (metric 23). Routes a crashed process left behind are removed on the next start.
- With no network at all, the server addresses are made unreachable rather than left to fall into the tunnel, where the engine would loop its traffic through itself. The tunnel is held, not torn down: tearing it down would let traffic out unprotected the moment a network returned.
- WireGuard's WebSocket carrier keeps one long connection, so it is started again on the new network while Xray and the tunnel interface stay up.
- 3 seconds after a move the client checks the route. A working route needs nothing and shows nothing; a broken one is replaced at once, with the same transport first, since the network rather than the transport was at fault.
- Control calls do not reuse connections, so none hangs on a connection the old network took with it.

`scripts/roaming-lab.sh` checks all of this in a Docker container with two networks: it moves the container mid-transfer, cuts every network, brings one back, and fails on any leak or on a site that does not load through the tunnel again within 20 seconds.

New servers are added from an invite by default, pasted as text or read from a QR image or pasted screenshot: the native `inspect_invite` and `decode_invite_qr` commands decode and verify it locally, and enrollment pins the invite's node key so the manifest must carry that node's signature (`InviteEnrollment`, `ControlEndpoint::parse_pinned`). A server URL and access code remain available under **Advanced**. The desktop shell retains the server URL, device name, protocol preference, access code, pinned node key, and server-issued access expiry after enrollment, and every restore re-enrolls with the same pin. On launch it transparently enrolls a fresh short-lived session from that saved profile instead of asking for the same code again. A normal application exit gets a bounded shutdown window to stop the local tunnel and release that short-lived session. Temporary control, capacity, or network failures retain the profile and retry in the background; only authoritative expiry, revocation, or the explicit **Forget server** action clears it. The shell displays the remaining server-enforced access time and never extends it locally.

Desktop appearance follows the operating-system color scheme by default and also supports persistent Light and Dark overrides. The protocol picker is an application-themed keyboard-accessible menu, avoiding platform-native popup colors that can make dark-mode choices unreadable. Connected state uses Ket's red liberated accent and focuses the map on the active server region; disconnected state removes every map transform and returns to a neutral, non-pulsing world view. A failed connect or cleanup opens a troubleshooting dialog. Its bounded report includes version, platform, engine readiness, protocol preference, phase, active transport identifier, issue code, retryability, elapsed time, and cleanup outcome. It excludes server URLs, access codes, session tokens, transport credentials, rendered configurations, and raw engine output, and applies a second defensive redaction pass before copying.

## Desktop service adapters

The privileged service delegates protocol logic to maintained upstream engines. The desktop process uses `BrokerTransportAdapter`, which authenticates to that service over loopback and never launches engines directly.

Hysteria2 uses its official UDP-capable [SOCKS5 mode](https://v2.hysteria.network/docs/advanced/Full-Client-Config/#socks5). VLESS + REALITY and HTTPS Stealth use strict Xray-core loopback SOCKS configurations; Stealth selects XHTTP `packet-up` with browser-fingerprinted, certificate-verified TLS. Shadowsocks 2022 uses the official shadowsocks-rust `sslocal` in TCP+UDP SOCKS mode with a strict SIP022 profile. WireGuard TLS uses Xray's userspace WireGuard outbound connected to a loopback UDP wstunnel client with pinned public SNI, certificate verification, Host header, and WebSocket path. Those engines feed one shared, pinned `tun2proxy` lifecycle for full-route TUN and [virtual-DNS](https://github.com/tun2proxy/tun2proxy#manual-setup) ownership. OpenVPN uses its native full-route TUN behind a loopback stunnel carrier, with independent certificate verification at both layers and a password-protected management channel for readiness. Ket does not reimplement any tunnel protocol or its cryptography.

- TLS verification is mandatory and uses the server-provided SNI.
- Each adapter resolves the server before route setup, pins the engine to the selected IP, and excludes every resolved server IP from full IPv4/IPv6 routes, preventing both a routing loop and data-plane DNS outside the tunnel.
- The bridge runs with automated system setup and virtual DNS. Hostname queries are intercepted inside the TUN and delivered to the selected SOCKS engine as names for remote resolution instead of being forwarded to a local or LAN resolver.
- Only known transport options are accepted. Unknown fields, missing credentials, invalid Gecko bounds, invalid REALITY or SIP022 keys, unsafe XHTTP paths, unsupported flows/modes, extra secrets, and downgrade-shaped fields fail closed.
- BBR defaults are preserved; the client does not invent unsafe bandwidth values.
- Configuration files are created in a private directory with mode `0600` on Unix, zeroed in the writer buffer, and removed after the complete process group and route are ready.
- Raw engine output is discarded after extracting an allowlisted diagnostic category, preventing share URIs or credentials from entering app logs. The full-route bridge continuously drains stderr with bounded memory and reports only fixed permission, TUN, DNS, route, driver, conflict, device, network, or missing-component categories. A post-start bridge failure reaches both the client issue and the privileged service journal without copying raw output.
- The child process is supervised and killed on explicit disconnect, terminal failure, service shutdown, or desktop heartbeat expiry.

The desktop package bundles verified Hysteria, Xray, `sslocal`, wstunnel, OpenVPN, stunnel, and `tun2proxy` binaries with the service installer. The GUI deliberately does not self-elevate. The SOCKS-backed transports carry TCP and UDP rather than arbitrary IP protocols such as ICMP; OpenVPN owns a native IP TUN instead.

## Integration sketch

```rust,no_run
use std::{sync::Arc, time::Duration};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, ControlEndpoint, HttpControlPlane, KetClient,
    SelectionPolicy,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = KetClient::new(
    ControlEndpoint::parse("https://ket.example.com")?,
    "Aadhish's workstation",
    Arc::new(HttpControlPlane::new()?),
    vec![Arc::new(BrokerTransportAdapter::new(BrokerConfig::from_env()?))],
    SelectionPolicy::default(),
)?;

let mut snapshots = client.subscribe();
client.enroll("A2345678901234567890123456789012").await?;
client.connect().await?;
let maintenance = client.spawn_maintenance(Duration::from_secs(15));

snapshots.changed().await?;
let ui_state = snapshots.borrow().clone();

client.disconnect().await?;
maintenance.shutdown().await;
# let _ = ui_state;
# Ok(())
# }
```
