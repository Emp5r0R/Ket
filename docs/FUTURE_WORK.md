# Future work

## Completed v0.2 boundary

The Ket v0.2 implementation scope is complete as of 2026-07-26. It includes the Rust control plane, the one-command Docker installer, lease-scoped grants and revocation, Linux and Windows desktop clients and privilege broker, Android `VpnService`, diagnostics, saved expiring profiles, automatic and explicitly locked protocol selection, and six maintained data-plane integrations:

- Hysteria2 with authenticated HTTP control and Salamander/Gecko options.
- VLESS + REALITY through Xray-core.
- HTTPS Stealth using VLESS + XHTTP/TLS through a CDN-capable edge.
- Shadowsocks 2022 through shadowsocks-rust.
- WireGuard through a WebSocket/TLS carrier.
- OpenVPN through certificate-pinned stunnel-compatible TLS.

The source gates cover strict profile parsing, credential isolation, lifecycle and revocation, fallback/manual locking, second-connect DNS recovery, control-only outage handling, full-route ownership, Android native payloads, and package installers. Real local engine traffic is exercised for XHTTP/TLS and Shadowsocks, and OpenVPN completes a certificate-pinned scoped handshake. Owner-signed Android restricted-network routes are already proven for REALITY, XHTTP/TLS Stealth, and Shadowsocks.

## Physical release qualification

These checks require hardware, network conditions, or owner-held signing identities. They are release-confidence work, not missing v0.2 implementation:

1. Repeat fail-closed Wi-Fi/mobile handover and engine-exit recovery on the current API 36 device.
2. Exercise Hysteria2, WireGuard TLS, and OpenVPN Android packet flow on restricted networks, including revocation and reconnect.
3. Run Hysteria2 and OpenVPN on physical API 26, plus the supported-protocol matrix on API 28 and current Android.
4. Exercise Doze exit, VPN-permission revoke, process death, reboot restoration, always-on, lockdown, connected-state DNS leak behavior, and sustained transfer.
5. Install the owner-signed Windows package on a clean Windows host and execute `WINDOWS_TESTING.md`.
6. Sign production Android, Windows, and Linux packages; add Windows code-signing and platform notarization where applicable.
7. Add long-running soak, upgrade, provider-limit, packet-loss, and censorship-simulation coverage across several regions and network operators.

## Future protocols

### IKEv2

`Ikev2` is currently a typed discovery identifier, not an executable adapter. A post-v0.2 implementation must use a maintained IKEv2 engine and preserve Ket's existing invariants: lease-scoped credentials, live revocation, traffic accounting, strict manual protocol locking, full-route DNS behavior, diagnostics, and Linux/Windows/Android parity. It must not be exposed in a client selector before those contracts and package payloads are implemented.

### XOR wrapping

`XorScrambled` is an extension identifier only. XOR provides no confidentiality or authentication and will never be offered as a standalone security protocol. Any implementation may only obfuscate an already authenticated and encrypted maintained transport, with downgrade-resistant negotiation and explicit server/client compatibility.

### Additional stealth transports

Ket's HTTPS Stealth and WireGuard TLS are independent self-hosted designs. They do not claim Proton Stealth wire compatibility because Proton's production server contract is not a public self-hostable interoperability target. Future transports should be added only when a maintained engine, reproducible package supply chain, server-side revocation, and all-client lifecycle behavior can be delivered together.
