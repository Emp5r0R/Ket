<p align="center">
  <img src="assets/ket-mark.svg" width="104" height="104" alt="Ket wave function mark">
</p>

# Ket

Ket is an anti-censorship connectivity platform in development. Its target is a Rust server, native Linux and Windows clients, and an Android client with a shared map-first experience and adaptive stealth transports.

> **Current state:** the Docker server and Linux/Windows clients can carry authenticated traffic through Hysteria2 and VLESS + REALITY, enforce lease revocation, and report per-session traffic. The Android client includes ranked startup fallback and session-preserving post-connect recovery on supported 64-bit ABIs. Production signing, Android physical-device packet-flow tests, and installer upgrade tests remain before this is a complete end-user VPN.

## Implemented now

- Rust workspace with shared, serializable client/server contracts.
- Shared Rust client controller with hardened HTTPS enrollment, UI-safe state snapshots, lease renewal, metrics refresh, bounded fallback, reconnect maintenance, and clean release.
- Map-first Tauri 2 desktop UI with secure enrollment, node geography, connection control, health/capacity telemetry, traffic history, and responsive Linux/Windows layouts.
- Authenticated loopback privilege broker with HMAC-SHA-256 installation identity, bounded framing, one-tunnel ownership, heartbeat expiry, and redacted diagnostics.
- Privileged desktop transport service with strict Hysteria2 and VLESS + REALITY validation, full-route TUN configuration, server-route exclusion, ephemeral mode-`0600` credentials, readiness detection, fallback, and supervised shutdown.
- Hardened `systemd` and Windows Service Control Manager installers with read-only desktop token access and non-destructive upgrades.
- Exactly 32-character access grants with Argon2-protected at-rest storage.
- Per-grant connection limits, global capacity, expiry, renewal, release, and revocation.
- Lease-scoped Hysteria2 credentials, HTTP authentication, traffic counters, online state, and connection kicks.
- Generated Hysteria2 2.10 server configuration with TLS, HTTP/3 masquerading, optional Salamander/Gecko obfuscation, and abuse-resistant ACLs.
- Generated Xray-core 26.3.27 VLESS + REALITY configuration with Vision, deterministic lease-scoped UUIDs, dynamic user reconciliation/revocation, traffic statistics, and abuse-resistant routing rules.
- Android Compose client with HTTPS enrollment, ranked VLESS + REALITY/Hysteria2 startup fallback, bounded post-connect recovery, foreground `VpnService` ownership, protected Hysteria QUIC sockets, server-route exclusion for Xray, maintained hev TUN-to-SOCKS forwarding, independent lease renewal, local traffic metrics, and fail-closed engine supervision.
- Typed discovery for Hysteria2, IKEv2, OpenVPN/stunnel, Shadowsocks 2022, VLESS XTLS Reality, WireGuard, stealth, and XOR-wrapped adapters.
- Country/city coordinates, health, capacity, CPU, memory, uptime, and Prometheus metrics.
- Atomic persistent state and graceful shutdown.
- Rootless, capability-free Docker control-plane image with a read-only root filesystem.

The component boundaries and remaining platform work are tracked in [the architecture](docs/ARCHITECTURE.md). The current endpoints are described in [the control API](docs/CONTROL_API.md), the shared controller in [the client-core guide](docs/CLIENT_CORE.md), the Android data plane in [the Android guide](docs/ANDROID.md), the desktop privilege boundary in [the tunnel-service guide](docs/TUNNEL_SERVICE.md), and data-plane deployment in the [Hysteria2](docs/HYSTERIA2.md) and [VLESS + REALITY](docs/XRAY_REALITY.md) guides.

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

Hysteria2 and REALITY can share port `443` because they use UDP and TCP respectively. To run the maintained dual-transport deployment, enable both sets of environment values and include both overlays:

```bash
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml ps
```

The control hostname may remain behind Cloudflare Tunnel, but each session manifest must advertise a direct server IP or DNS-only hostname for the raw data planes. Open stateful TCP `443` and UDP `443` in both the cloud network policy and host firewall; keep the control port closed publicly.

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

The Android project is under `apps/ket-android`. It consumes the same control contract as desktop and carries TCP/UDP packets through a local Hysteria2 or Xray SOCKS endpoint and the maintained hev TUN bridge. Android uses a platform-specific lifecycle adapter instead of embedding desktop route-management code.

For a local Android build, install Android SDK Platform 34 and Build Tools 34, then run `./packaging/build-android.sh`. It auto-detects the SDK used by Abyssal when present; set `KET_ANDROID_SDK` to override it. Gradle installs pinned NDK r27d when needed, while the build downloads and verifies Hysteria 2.10, Xray-core 26.3.27, and hev-socks5-tunnel 2.14.0. Xray publishes official Android payloads for `arm64-v8a` and `x86_64`; 32-bit builds retain Hysteria2. The generated APK is under `apps/ket-android/app/build/outputs/apk/debug/`.

Continuous integration is defined in `.github/workflows/ci.yml`: Rust formatting/tests/lints, desktop UI tests/build, native packages, Android debug packaging, and the control-plane container build run when their inputs change. Workflow changes and manual runs execute the complete matrix, while documentation-only changes avoid unnecessary builds.

## Delivery order

1. Sign the implemented Linux/Windows packages and exercise clean-install, upgrade, service-start, and uninstall paths on target machines.
2. Exercise both Android data planes, startup fallback, and post-connect recovery on physical API 26 and current devices, then add release signing and network-change/Doze tests.
3. Evaluate the next maintained transport only after the shipped dual-transport paths pass the release matrix.
4. Add soak, network-failure, upgrade, and censorship-simulation tests across the transport matrix.

Ket must use maintained protocol implementations and authenticated encryption. Obfuscation such as XOR is never treated as security on its own.
