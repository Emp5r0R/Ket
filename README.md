<p align="center">
  <img src="assets/ket-mark.svg" width="104" height="104" alt="Ket wave function mark">
</p>

# Ket

Ket is an anti-censorship connectivity platform in development. Its target is a Rust server, native Linux and Windows clients, and an Android client with a shared map-first experience and adaptive stealth transports.

> **Current state:** the Docker server and Linux/Windows/Android clients implement six authenticated data planes with lease revocation: Hysteria2, VLESS + REALITY, CDN-carried VLESS + XHTTP/TLS Stealth, Shadowsocks 2022, WireGuard over WebSocket/TLS, and OpenVPN over stunnel-compatible TLS. Hysteria2 and REALITY have carried traffic on a physical current arm64 Android device; the remaining transports have local code, engine, package, or container validation rather than restricted-network end-to-end results. Physical lifecycle and network tests remain before this is a complete end-user VPN.

## Implemented now

- Rust workspace with shared, serializable client/server contracts.
- Shared Rust client controller with hardened HTTPS enrollment, bounded and identity-bound control-response validation, UI-safe last-known-good state snapshots, lease renewal, metrics refresh, bounded fallback, persistent reconnect intent across fully blocked rounds, and clean release.
- Map-first Tauri 2 desktop UI with secure enrollment, node geography, connection control, health/capacity telemetry, traffic history, and responsive Linux/Windows layouts.
- Authenticated loopback privilege broker with HMAC-SHA-256 installation identity, bounded framing, one-tunnel ownership, heartbeat expiry, and redacted diagnostics.
- Privileged desktop transport service with strict Hysteria2, VLESS + REALITY, XHTTP/TLS Stealth, Shadowsocks 2022, WireGuard TLS, and OpenVPN/stunnel validation, full-route ownership, every resolved server IP excluded, ephemeral mode-`0600` credentials, readiness detection, fallback, and supervised shutdown.
- Hardened `systemd` and Windows Service Control Manager installers with read-only desktop token access and non-destructive upgrades.
- Exactly 32-character access grants with Argon2-protected at-rest storage.
- Per-grant connection limits, global capacity, expiry, renewal, release, and revocation.
- Lease-scoped Hysteria2 credentials, HTTP authentication, traffic counters, online state, and connection kicks.
- Generated Hysteria2 2.10 server configuration with TLS, HTTP/3 masquerading, optional Salamander/Gecko obfuscation, and abuse-resistant ACLs.
- Generated Xray-core 26.3.27 VLESS + REALITY and VLESS + XHTTP configurations with deterministic lease-scoped UUIDs, atomic multi-inbound reconciliation/revocation, traffic statistics, and abuse-resistant routing rules.
- Maintained `shadowsocks-rust` 1.24.0 SIP022 AEAD-2022 server manager with deterministic lease-scoped keys, crash-safe per-lease TCP/UDP ports, reconciliation/revocation, and abuse-resistant ACLs.
- Maintained WireGuard kernel server and Xray userspace clients carried through checksum-pinned `wstunnel` 10.6.2, with deterministic lease-scoped keys, preshared keys, addresses, peer reconciliation/revocation, and certificate-verified WSS.
- Checksum-pinned OpenVPN 2.7.5 inside stunnel 5.79 TLS, with two independent certificate chains, `tls-crypt`, scoped username/password authentication, session reconciliation/revocation, traffic counters, and a capability-limited Linux agent.
- Android Compose client with a real Natural Earth server map, coordinate-based location marker, node health/capacity/CPU/memory/uptime telemetry, bounded and identity-bound HTTPS control responses, Keystore-sealed durable credentials, process/reboot-safe lease restoration, system always-on entry, ranked OpenVPN TLS, WireGuard TLS, Shadowsocks 2022, XHTTP/TLS Stealth, VLESS + REALITY, and Hysteria2 startup fallback, bounded post-connect and underlying-network-change recovery, foreground `VpnService` ownership, collision-safe dual-stack VPN DNS, a fail-closed replacement-route guard, Doze-aware lease validation, graceful VPN-permission revocation, protected carrier sockets, server-route exclusion for upstream engines, maintained hev TUN-to-SOCKS forwarding, OpenVPN management/TUN descriptor handoff, local traffic metrics, and engine supervision.
- Fail-closed Android release signing with operator-supplied version metadata and signer-certificate pinning; CI exercises the complete release path with a disposable identity that is never published as a trusted release.
- Typed discovery identifiers remain for future IKEv2 and XOR-wrapped adapters. These identifiers are not executable protocol support.
- Country/city coordinates, health, capacity, CPU, memory, uptime, and Prometheus metrics.
- Fail-fast server configuration that structurally validates the public URL and bounds node, location, transport, TLS-name, and option metadata before any manifest is issued.
- Atomic persistent state and graceful shutdown.
- Rootless, capability-free Docker control-plane image with a read-only root filesystem.

| Transport | Server | Linux/Windows | Android | Deployment status |
| --- | --- | --- | --- | --- |
| Hysteria2 + Salamander/Gecko | Implemented | Implemented | Implemented | Physical arm64 traffic verified |
| VLESS + REALITY | Implemented | Implemented | Implemented on 64-bit | Physical arm64 traffic verified |
| HTTPS Stealth (VLESS + XHTTP/TLS) | Implemented | Implemented | Implemented on 64-bit | Local validation complete; restricted-network physical gate pending |
| Shadowsocks 2022 | Implemented | Implemented | Implemented on 64-bit API 28+ | Local upstream-engine validation complete; restricted-network physical gate pending |
| WireGuard over WebSocket/TLS | Implemented | Implemented | Implemented on arm64 API 28+ | Local engine/package validation complete; kernel-server and restricted-network gates pending |
| OpenVPN over stunnel TLS | Implemented | Implemented | Implemented on API 26+ | Local code/engine/package/container validation; deployment and Android physical traffic pending |
| IKEv2 | Identifier only | Not implemented | Not implemented | Future adapter |
| XOR scrambling | Obfuscation identifier only | Not standalone security | Not standalone security | May only wrap authenticated encryption |

The component boundaries and remaining platform work are tracked in [the architecture](docs/ARCHITECTURE.md). The current endpoints are described in [the control API](docs/CONTROL_API.md), the shared controller in [the client-core guide](docs/CLIENT_CORE.md), the Android data plane in [the Android guide](docs/ANDROID.md), the desktop privilege boundary in [the tunnel-service guide](docs/TUNNEL_SERVICE.md), and data-plane deployment in the [Hysteria2](docs/HYSTERIA2.md), [VLESS + REALITY](docs/XRAY_REALITY.md), [HTTPS Stealth](docs/XHTTP_STEALTH.md), [Shadowsocks 2022](docs/SHADOWSOCKS2022.md), [WireGuard TLS](docs/WIREGUARD_TLS.md), and [OpenVPN over stunnel](docs/OPENVPN_STUNNEL.md) guides.

## Run with Docker

```bash
cp .env.example .env
openssl rand -base64 48
# Put the generated value in KET_ADMIN_TOKEN, then set the public URL and location.
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose up --build -d
curl --fail http://127.0.0.1:8787/healthz
```

The Compose default publishes only on loopback. Put a TLS reverse proxy in front of port `8787`, or explicitly set `KET_PUBLISH_ADDRESS` when a different binding is required. Do not expose an unencrypted control API over the public internet, and deny all public requests to `/internal/`; that namespace is reserved for private data-plane callbacks.

The shell preflight rejects malformed operator-facing URL and node fields before Compose starts. `ket-server` repeats the authoritative structured validation at process startup, including HTTPS-or-loopback URL policy, map metadata, transport count and identifier bounds, endpoint/TLS names, and option maps, so it cannot emit a manifest that Ket clients reject for shape.

To enable the Hysteria2 data plane, install a valid certificate under `secrets/tls`, set `KET_HYSTERIA_ENABLED=true` and the related Hysteria values in `.env`, then use the overlay:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml up --build -d
```

Before starting the overlay, validate the fully rendered configuration (this
also catches missing `.env` values and malformed port mappings):

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml config --quiet
```

The overlay keeps the control plane on its private Compose network and publishes
only Hysteria2's UDP listener. The generated Hysteria configuration is written
with mode `0600` into the shared `ket-dataplane` volume; do not bind-mount that
volume or expose the internal authentication and statistics endpoints.

This publishes Hysteria2 on UDP `443` by default. The hostname must reach the server over UDP; normal Cloudflare orange-cloud HTTP proxying is not a UDP tunnel. Use a DNS-only record or a deliberately configured Layer 4 product compatible with unmodified Hysteria packets.

To enable the VLESS + REALITY server data plane, generate the X25519 and server-only credential keys described in [the deployment guide](docs/XRAY_REALITY.md), set `KET_XRAY_ENABLED=true`, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.xray.yaml config --quiet
docker compose -f compose.yaml -f compose.xray.yaml up --build -d
```

This publishes raw TCP `443` by default. Its hostname must resolve directly to the server through a DNS-only record; an ordinary Cloudflare Tunnel or orange-cloud HTTP proxy does not forward unmodified VLESS + REALITY traffic. The pinned Xray image is a multi-architecture manifest, so Docker selects the native `linux/amd64` or `linux/arm64` image for the host.

To enable the TLS-shaped Stealth path, set `KET_XHTTP_ENABLED=true`, configure the public Cloudflare hostname and an unguessable `KET_XHTTP_PATH`, then include the XHTTP overlay:

```bash
docker compose -f compose.yaml -f compose.xhttp.yaml config --quiet
docker compose -f compose.yaml -f compose.xhttp.yaml up --build -d
```

The XHTTP origin is published only on host loopback port `8445`. Route only its configured path from Cloudflare Tunnel to that origin and keep the remaining hostname routes pointed at the control API. The public client connection is certificate-verified TLS and browser-fingerprinted HTTP; the private origin hop is plain HTTP inside the authenticated outbound Cloudflare Tunnel. Exact ingress configuration and validation commands are in [the HTTPS Stealth guide](docs/XHTTP_STEALTH.md).

To enable Shadowsocks 2022, generate its independent credential key, configure an inclusive port range containing at least `KET_MAX_SESSIONS` ports, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.shadowsocks.yaml config --quiet
docker compose -f compose.yaml -f compose.shadowsocks.yaml up --build -d
```

Each active lease owns one stable TCP+UDP port and a distinct SIP022 key. Open the complete configured range for both TCP and UDP in the cloud security list and host firewall. The public hostname must be a direct DNS-only record; an ordinary Cloudflare orange-cloud proxy or Cloudflare Tunnel does not carry native Shadowsocks. The UDP manager stays isolated on the private Compose network. Exact key, range, and validation commands are in [the Shadowsocks 2022 guide](docs/SHADOWSOCKS2022.md).

To enable WireGuard TLS, generate its server key pair and two independent server-only secrets, set an unguessable WebSocket path prefix, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.wireguard.yaml config --quiet
docker compose -f compose.yaml -f compose.wireguard.yaml up --build -d
```

The overlay runs a capability-limited WireGuard kernel agent and an unprivileged `wstunnel` origin published only on host loopback port `8446`. Route the configured WebSocket path from Cloudflare Tunnel to `http://localhost:8446`; clients connect to certificate-verified `wss://` on the public hostname. This path needs no public OCI ingress rule. It is a self-hosted transport inspired by the same general WireGuard-over-TLS strategy as Proton Stealth, but it is not Proton-compatible. Exact setup and validation commands are in [the WireGuard TLS guide](docs/WIREGUARD_TLS.md).

To enable OpenVPN over stunnel, create the OpenVPN and stunnel PKI material, generate two independent server tokens, and start its overlay:

```bash
docker compose -f compose.yaml -f compose.openvpn.yaml config --quiet
docker compose -f compose.yaml -f compose.openvpn.yaml up --build -d
```

The outer stunnel connection is certificate-verified TLS and the inner OpenVPN connection independently verifies its server certificate and `tls-crypt` key. This is direct generic TLS, not HTTP, so use a DNS-only record and open the configured TCP port. The OpenVPN agent owns only `NET_ADMIN` and `/dev/net/tun`; its management API and the authentication callback remain on a private Compose network. Exact PKI, ingress, port-collision, and revocation details are in [the OpenVPN/stunnel guide](docs/OPENVPN_STUNNEL.md).

Hysteria2 and REALITY can share port `443` because they use UDP and TCP respectively. OpenVPN/stunnel is also TCP and therefore needs a different host bind address or public port from REALITY. HTTPS Stealth and WireGuard TLS use separate Cloudflare hostnames and loopback-only origins, while Shadowsocks uses its own TCP+UDP range. To run all six server/desktop transports, enable every environment section, assign OpenVPN a non-conflicting TCP listener, and include all overlays:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml ps
```

The control, XHTTP, and WireGuard TLS hostnames may remain behind Cloudflare Tunnel, but Hysteria2, REALITY, Shadowsocks, and OpenVPN/stunnel session profiles must advertise a direct server IP or DNS-only hostname. Open only the exact raw-transport ports in both the cloud network policy and host firewall; keep all Cloudflare Tunnel origin ports closed publicly.

Create an access grant:

```bash
set -a; . ./.env; set +a
curl --fail-with-body \
  --request POST \
  --header "Authorization: Bearer ${KET_ADMIN_TOKEN}" \
  --header "Content-Type: application/json" \
  --data '{"label":"Personal devices","max_connections":5,"expires_at_epoch_seconds":null}' \
  http://127.0.0.1:8787/v1/admin/access-grants
```

## Develop

The server, shared contracts, client core, and tunnel service support both `amd64` and `arm64`, including Oracle Ampere hosts. Building the Tauri desktop app additionally requires Tauri's platform webview development packages.

```bash
cargo fmt --all -- --check
cargo test --workspace --exclude ket-desktop
cargo clippy --workspace --exclude ket-desktop --all-targets --all-features -- -D warnings
cargo build --release --package ket-server
```

`/media/n_emperor/Aadhish/gradle-home` is a Gradle user-home cache, not a Gradle executable. Use it with the repository wrapper:

```bash
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home ./gradlew build
```

The Android project is under `apps/ket-android`. It consumes the same control contract as desktop and carries TCP/UDP packets through a local Hysteria2, Xray, Shadowsocks, or WireGuard-over-WSS SOCKS endpoint and the maintained hev TUN bridge. Android uses a platform-specific lifecycle adapter instead of embedding desktop route-management code.

For a local Android build, install Android SDK Platform 34 and Build Tools 34, then run `./packaging/build-android.sh debug`. It auto-detects the SDK used by Abyssal when present; set `KET_ANDROID_SDK` to override it. Gradle installs pinned NDK r27d when needed, while the build downloads and verifies Hysteria 2.10, Xray-core 26.3.27, shadowsocks-rust 1.24.0, wstunnel 10.6.2, OpenVPN for Android 0.7.64, and hev-socks5-tunnel 2.14.0. OpenVPN and Hysteria payloads are included for all four ABIs; Xray and Shadowsocks are included for `arm64-v8a` and `x86_64`; the official Android wstunnel payload is currently arm64-only. Shadowsocks and WireGuard TLS require API 28 or newer, and unsupported devices retain ranked fallback. The generated APK is under `apps/ket-android/app/build/outputs/apk/debug/`. The signer-pinned release procedure is in [the release checklist](docs/RELEASE.md).

Continuous integration is defined in `.github/workflows/ci.yml`: Rust formatting/tests/lints, desktop UI tests/build, native packages, Android debug packaging, and the control-plane container build run when their inputs change. Workflow changes and manual runs execute the complete matrix, while documentation-only changes avoid unnecessary builds.

## Delivery order

1. Sign the implemented Linux/Windows packages and exercise their signed artifacts on target machines; the unsigned Linux and Windows installer, service, reinstall, and removal lifecycles are CI-gated.
2. Retest fail-closed network handover on the current API 36 device, repeat the Android matrix on physical API 26, then complete Doze, revoke, connected-state DNS-leak, and owner-signed installation tests.
3. Exercise the implemented Android process/reboot restoration and always-on/lockdown lifecycle on physical API 36 and API 26 devices.
4. Exercise HTTPS Stealth, Shadowsocks 2022, WireGuard TLS, and OpenVPN/stunnel on restricted networks, including desktop startup fallback and live revocation.
5. Exercise the Android OpenVPN management/TUN bridge against the deployed stunnel listener, including restricted-network fallback, revocation, and network-change recovery.
6. Add soak, network-failure, upgrade, and censorship-simulation tests across the transport matrix.
7. Implement the remaining IKEv2 adapter without weakening the existing authenticated fallback contract.

Ket must use maintained protocol implementations and authenticated encryption. Obfuscation such as XOR is never treated as security on its own.
