<p align="center">
  <img src="assets/ket-mark.svg" width="104" height="104" alt="Ket wave function mark">
</p>

# Ket

Ket is an anti-censorship connectivity platform in development. Its target is a Rust server, native Linux and Windows clients, and an Android client with a shared map-first experience and adaptive stealth transports.

> **Current state:** the Docker server can carry authenticated traffic through Hysteria2 and report per-session traffic. The Rust client core, map-first Tauri desktop shell, authenticated privilege broker, and Linux/Windows service installers are implemented. Signed desktop bundles with a verified engine payload and the Android `VpnService` client are still required before this is a complete end-user VPN.

## Implemented now

- Rust workspace with shared, serializable client/server contracts.
- Shared Rust client controller with hardened HTTPS enrollment, UI-safe state snapshots, lease renewal, metrics refresh, bounded fallback, reconnect maintenance, and clean release.
- Map-first Tauri 2 desktop UI with secure enrollment, node geography, connection control, health/capacity telemetry, traffic history, and responsive Linux/Windows layouts.
- Authenticated loopback privilege broker with HMAC-SHA-256 installation identity, bounded framing, one-tunnel ownership, heartbeat expiry, and redacted diagnostics.
- Privileged Hysteria2 service adapter with strict TLS, full-route TUN configuration, server-route exclusion, fail-closed option validation, ephemeral mode-`0600` credentials, readiness detection, and supervised shutdown.
- Hardened `systemd` and Windows Service Control Manager installers with read-only desktop token access and non-destructive upgrades.
- Exactly 32-character access grants with Argon2-protected at-rest storage.
- Per-grant connection limits, global capacity, expiry, renewal, release, and revocation.
- Lease-scoped Hysteria2 credentials, HTTP authentication, traffic counters, online state, and connection kicks.
- Generated Hysteria2 2.10 server configuration with TLS, HTTP/3 masquerading, optional Salamander/Gecko obfuscation, and abuse-resistant ACLs.
- Android Compose client scaffold with the same map-first connection surface and an isolated `VpnService` TUN lifecycle boundary.
- Typed discovery for Hysteria2, IKEv2, OpenVPN/stunnel, Shadowsocks 2022, VLESS XTLS Reality, WireGuard, stealth, and XOR-wrapped adapters.
- Country/city coordinates, health, capacity, CPU, memory, uptime, and Prometheus metrics.
- Atomic persistent state and graceful shutdown.
- Rootless, capability-free Docker control-plane image with a read-only root filesystem.

The component boundaries and remaining platform work are tracked in [the architecture](docs/ARCHITECTURE.md). The current endpoints are described in [the control API](docs/CONTROL_API.md), the shared controller in [the client-core guide](docs/CLIENT_CORE.md), the desktop privilege boundary in [the tunnel-service guide](docs/TUNNEL_SERVICE.md), and the first data-plane deployment in [the Hysteria2 guide](docs/HYSTERIA2.md).

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

For the upcoming Android project, `/media/n_emperor/Aadhish/gradle-home` is a Gradle user-home cache, not a Gradle executable. Use it as follows once the repository has a wrapper:

```bash
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home ./gradlew build
```

The Android project is under `apps/ket-android`. It provides the end-user shell, HTTPS enrollment against `POST /v1/sessions`, Android VPN permission flow, and TUN ownership boundary; binding the Rust client core and transport engine is the next integration step.

For a local Android build, install Android SDK Platform 34 and Build Tools 34, then run `./packaging/build-android.sh`. It auto-detects the SDK used by Abyssal when present; set `KET_ANDROID_SDK` to override it. The generated APK is under `apps/ket-android/app/build/outputs/apk/debug/`.

Continuous integration is defined in `.github/workflows/ci.yml`: Rust formatting/tests/lints, desktop UI tests/build, Android debug packaging, and the control-plane container build run on every push and pull request.

## Delivery order

1. Bundle the pinned Hysteria engine into the implemented Linux/Windows desktop and service packages, then sign and exercise upgrade paths for each artifact.
2. Ship the Android `VpnService` adapter with Compose UI and parity for enrollment, metrics, fallback, and diagnostics.
3. Integrate the next maintained transport engine behind the existing data-plane boundary.
4. Add soak, network-failure, upgrade, and censorship-simulation tests across the transport matrix.

Ket must use maintained protocol implementations and authenticated encryption. Obfuscation such as XOR is never treated as security on its own.
