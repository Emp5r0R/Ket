# Features

Everything Ket implements today, and the qualification status of each transport. Setup instructions are in [SERVER_INSTALL.md](SERVER_INSTALL.md).

## Since v1.1.0 (on `main`, not yet released)

- Android split tunneling: choose all apps, only chosen apps, or all except chosen apps ([ANDROID.md](ANDROID.md#apps-that-use-ket)).
- `ket doctor` flags a Cloudflare Tunnel that sends the Ket hostname past the edge to the control plane.

## v1.1.0

- A probe-resistant HTTPS edge: only client session routes and exact secret transport paths are reachable, and scanners see one neutral page ([CONTROL_API.md](CONTROL_API.md#public-exposure)).
- Signed invites: one `ket:invite:` string or QR code replaces the server URL and access code, and pins the node's Ed25519 key so enrollment proves it reached the right server ([INVITE_CAPSULE.md](INVITE_CAPSULE.md)).
- Invite entry on desktop (paste, QR image, pasted screenshot) and Android (paste, camera, QR image); `sudo ket invite` with a terminal QR on servers.
- One-command server setup for any modern systemd Linux with automatic Cloudflare detection ([SERVER_INSTALL.md](SERVER_INSTALL.md)).
- The `ket` server command: guided invites, invite list and revoke, status, a troubleshooting `doctor`, logs, backups, key rotation, update, rollback, and uninstall ([SERVER_INSTALL.md](SERVER_INSTALL.md#managing-the-server)).
- An update lifecycle: every release is rated for apps and servers; servers apply security fixes on their own with a health-checked, self-reverting swap, and tell each app whether its update is optional, recommended, security (with a deadline), or required ([UPDATES.md](UPDATES.md)).
- Published multi-architecture server images on release, so installs and updates need not compile on the VPS.
- A smaller Android download: an arm64 APK, about half the size, next to the universal APK.

## v1.0

- Rust workspace with shared, serializable client/server contracts.
- Shared Rust client controller with hardened HTTPS enrollment, bounded and identity-bound control-response validation, UI-safe last-known-good state snapshots, lease renewal, metrics refresh, bounded fallback, same-profile verified-address recovery across temporary DNS failure, independent routed-Internet checks before control outages trigger tunnel recovery, explicit manual-retry semantics, and clean release.
- Map-first Tauri 2 desktop UI with automatically restored time-bounded profiles, explicit Forget and expiry cleanup, remaining-access display, system/light/dark themes, a themed keyboard-accessible explicit or ranked Automatic protocol menu, an in-app guide for every implemented protocol, connected-region map focus, an untransformed neutral disconnected world view, copyable redacted failure diagnostics, node geography, connection control, health/capacity telemetry, traffic history, and responsive Linux/Windows layouts.
- Authenticated loopback privilege broker with HMAC-SHA-256 installation identity, bounded framing, one-tunnel ownership, heartbeat expiry, and redacted diagnostics.
- Privileged desktop transport service with strict Hysteria2/Hysteria XOR, VLESS + REALITY, XHTTP/TLS Stealth, Shadowsocks 2022, WireGuard TLS, and OpenVPN/stunnel validation, full-route ownership, every resolved server IP excluded, ephemeral mode-`0600` credentials, readiness detection, fallback, and supervised shutdown.
- Hardened `systemd` and Windows Service Control Manager installers with read-only desktop token access and non-destructive upgrades.
- Exactly 32-character access grants with Argon2-protected at-rest storage and server-derived lifetimes in minutes.
- Per-grant connection limits, global capacity, expiry, renewal, release, and revocation.
- Lease-scoped Hysteria2 credentials, HTTP authentication, traffic counters, online state, and connection kicks.
- Generated Hysteria2 2.10 server configuration with TLS, HTTP/3 masquerading, optional Salamander/Gecko obfuscation, and abuse-resistant ACLs.
- Generated Xray-core 26.3.27 VLESS + REALITY and VLESS + XHTTP configurations with deterministic lease-scoped UUIDs, atomic multi-inbound reconciliation/revocation, traffic statistics, and abuse-resistant routing rules.
- Maintained `shadowsocks-rust` 1.24.0 SIP022 AEAD-2022 server manager with deterministic lease-scoped keys, crash-safe per-lease TCP/UDP ports, reconciliation/revocation, and abuse-resistant ACLs.
- Maintained WireGuard kernel server and Xray userspace clients carried through checksum-pinned `wstunnel` 10.6.2, with deterministic lease-scoped keys, preshared keys, addresses, peer reconciliation/revocation, and certificate-verified WSS.
- Checksum-pinned OpenVPN 2.7.5 inside stunnel 5.80 TLS, with two independent certificate chains, `tls-crypt`, scoped username/password authentication, session reconciliation/revocation, traffic counters, and a capability-limited Linux agent.
- Android Compose client with system/light/dark themes, subtle state motion, a visible saved-profile state and explicit Forget action, strict explicit protocol choice or ranked Automatic fallback, an in-app guide for every implemented protocol, a Natural Earth map that focuses on the connected region and returns to a neutral world view after cleanup, copyable secret-redacted lifecycle diagnostics, node health/capacity/CPU/memory/uptime telemetry, remaining-access display, bounded and identity-bound HTTPS control responses, Keystore-sealed durable credentials including the selected preference and access expiry, automatic profile erasure at expiry, process/reboot-safe lease restoration, system always-on entry, ranked OpenVPN TLS, WireGuard TLS, Shadowsocks 2022, XHTTP/TLS Stealth, VLESS + REALITY, Hysteria XOR, and Hysteria2 startup, bounded post-connect and underlying-network-change recovery, foreground `VpnService` ownership, collision-safe dual-stack VPN DNS, a fail-closed replacement-route guard, Doze-aware lease validation, graceful VPN-permission revocation, protected carrier sockets, server-route exclusion for upstream engines, maintained hev TUN-to-SOCKS forwarding, OpenVPN management/TUN descriptor handoff, local traffic metrics, and engine supervision.
- Fail-closed Android release signing with operator-supplied version metadata and signer-certificate pinning; CI exercises the complete release path with a disposable identity that is never published as a trusted release.
- IKEv2 remains a typed future discovery identifier. Hysteria XOR is executable on every client when the server enables Hysteria's maintained Salamander or Gecko layer; it is packet obfuscation over authenticated encrypted QUIC, never standalone security.
- Country/city coordinates, health, capacity, CPU, memory, uptime, and Prometheus metrics.
- Fail-fast server configuration that structurally validates the public URL and bounds node, location, transport, TLS-name, and option metadata before any manifest is issued.
- Atomic persistent state and graceful shutdown.
- Rootless, capability-free Docker control-plane image with a read-only root filesystem.

| Transport | Server | Linux/Windows | Android | Deployment status |
| --- | --- | --- | --- | --- |
| Hysteria2 | Implemented | Implemented | Implemented | Deployed authentication exercised; restricted-network routed-traffic gate pending |
| Hysteria XOR (Salamander/Gecko) | Implemented | Implemented | Implemented | Maintained Hysteria obfuscation over authenticated encrypted QUIC; same UDP reachability requirement |
| VLESS + REALITY | Implemented | Implemented | Implemented on 64-bit | Owner-signed Android restricted-network egress verified on Oracle ARM64 |
| HTTPS Stealth (VLESS + XHTTP/TLS) | Implemented | Implemented | Implemented on 64-bit | Owner-signed Android restricted-network route verified through the production HTTPS edge |
| Shadowsocks 2022 | Implemented | Implemented | Implemented on 64-bit API 28+ | Public TCP+UDP lifecycle and owner-signed Android restricted-network route verified |
| WireGuard over WebSocket/TLS | Implemented | Implemented | Implemented on arm64 API 28+ | Oracle ARM64 kernel deployment, enrollment, and peer revocation verified; restricted-network traffic pending |
| OpenVPN over stunnel TLS | Implemented | Implemented | Implemented on API 26+ | Public certificate-pinned nested-TLS handshake and telemetry verified on Oracle ARM64; revocation lifecycle verified; Android physical traffic pending |
| IKEv2 | Identifier only | Not implemented | Not implemented | Future adapter |

The completed v1.0 boundary and remaining qualification/protocol work are tracked in [future work](FUTURE_WORK.md) and [the architecture](ARCHITECTURE.md). The current endpoints are described in [the control API](CONTROL_API.md), the shared controller in [the client-core guide](CLIENT_CORE.md), the Android data plane in [the Android guide](ANDROID.md), the desktop privilege boundary in [the tunnel-service guide](TUNNEL_SERVICE.md), and the real-hardware procedure in the [Windows package test plan](WINDOWS_TESTING.md). Data-plane deployment is covered by the [Hysteria2](HYSTERIA2.md), [VLESS + REALITY](XRAY_REALITY.md), [HTTPS Stealth](XHTTP_STEALTH.md), [Shadowsocks 2022](SHADOWSOCKS2022.md), [WireGuard TLS](WIREGUARD_TLS.md), and [OpenVPN over stunnel](OPENVPN_STUNNEL.md) guides.

Stable artifact checksums, Android ownership signing, and detached OpenPGP verification are covered by the [release-signing guide](RELEASE_SIGNING.md).

## Future work

The v1.0 product boundary is complete. Physical release qualification, soak coverage, and future IKEv2 work are intentionally kept in [the future-work register](FUTURE_WORK.md). Ket will continue to use maintained protocol implementations and authenticated encryption; XOR is never treated as security on its own.
