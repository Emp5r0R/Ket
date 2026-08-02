<p align="center">
  <img src="assets/ket-mark.svg" width="104" height="104" alt="Ket Psi wavefront mark">
</p>

# Ket

Ket is an anti-censorship connectivity platform with a Rust server, native Linux and Windows clients, and an Android client sharing a map-first experience and adaptive stealth transports.

> **Project status:** the v1.0 implementation scope is complete as of 2026-07-28. The Docker server and Linux/Windows/Android clients implement six authenticated, revocable data planes plus selectable Hysteria XOR obfuscation: Hysteria2, VLESS + REALITY, CDN-carried VLESS + XHTTP/TLS Stealth, Shadowsocks 2022, WireGuard over WebSocket/TLS, and OpenVPN over stunnel-compatible TLS. REALITY, XHTTP/TLS Stealth, and Shadowsocks 2022 have passed the routed HTTPS gate on a restricted network using an owner-signed physical arm64 Android client. Remaining hardware, wider-network, soak, and future IKEv2 work is tracked separately and is not represented as completed evidence.

## Bring your own server

Ket needs one public VPS and one Ket client. Start with a fresh `amd64` or `arm64` server running Debian 12+ or Ubuntu 22.04+; 2 CPU cores, 2 GiB RAM, and 10 GiB free disk are recommended. Keep SSH access, then add these **stateful inbound** rules in the VPS provider's firewall or security group:

| Protocol | Port | Used by |
| --- | --- | --- |
| TCP | `80` | Let's Encrypt issuance and renewal |
| TCP | `443` | Control API, VLESS + REALITY, HTTPS Stealth, WireGuard TLS |
| UDP | `443` | Hysteria2 and Hysteria XOR |
| TCP | `9443` | OpenVPN over TLS |
| TCP+UDP | `20000-20031` | Shadowsocks 2022 with the default 32-session capacity |

Use source `0.0.0.0/0`. Do not expose TCP `8787`, `8445`, or `8446`. Ket adds matching rules when UFW is already active, but it cannot edit an Oracle Cloud security list, AWS security group, Google Cloud firewall, or another provider firewall.

On a fresh VPS, install `curl` once:

```bash
sudo apt-get update && sudo apt-get install -y ca-certificates curl
```

Then choose exactly one setup.

**No domain:** Ket detects the public IPv4 address and creates a certificate-backed `sslip.io` hostname automatically. This needs no DNS account, but a domain you control is preferred for a long-lived server because automatic mode depends on an external wildcard DNS service. Restricted DNS resolvers may block wildcard-IP hostnames; use your own domain when that affects intended clients.

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | sudo bash -s -- \
  --mode auto \
  --email operator@example.com
```

**Your own domain, without Cloudflare proxying:** create an `A` record such as `ket.example.com` pointing directly to the VPS, wait for it to resolve, then run:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | sudo bash -s -- \
  --mode direct \
  --domain ket.example.com \
  --email operator@example.com
```

**Cloudflare:** create `ket.example.com` as a **Proxied** record and `direct-ket.example.com` as a **DNS only** record, both pointing to the VPS. Set SSL/TLS to **Full (strict)** and keep WebSockets enabled, then run:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/packaging/install-server.sh | sudo bash -s -- \
  --mode cloudflare \
  --domain ket.example.com \
  --direct-host direct-ket.example.com \
  --email operator@example.com
```

The installer validates DNS, ports, OS, CPU architecture, Docker, certificates, and every rendered transport before starting Ket. It prints the exact provider-firewall checklist before provisioning and returns one 32-character access code after readiness passes. Preview without changing the server by adding `--plan`; use `--max-sessions 64` when a larger Shadowsocks port range is already open.

After setup, download the appropriate signed client from the [latest stable Ket release](https://github.com/Emp5r0R/Ket/releases/tag/v1.0.0): `.deb` for Debian/Ubuntu desktop, `.exe` for Windows, or the universal `.apk` for Android. Open the client, enter the printed `https://...` server URL and access code, then connect. Verify downloads with the release's signed `SHA256SUMS` using the [release-signing guide](docs/RELEASE_SIGNING.md).

## Implemented now

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
- Checksum-pinned OpenVPN 2.7.5 inside stunnel 5.79 TLS, with two independent certificate chains, `tls-crypt`, scoped username/password authentication, session reconciliation/revocation, traffic counters, and a capability-limited Linux agent.
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

The completed v1.0 boundary and remaining qualification/protocol work are tracked in [future work](docs/FUTURE_WORK.md) and [the architecture](docs/ARCHITECTURE.md). The current endpoints are described in [the control API](docs/CONTROL_API.md), the shared controller in [the client-core guide](docs/CLIENT_CORE.md), the Android data plane in [the Android guide](docs/ANDROID.md), the desktop privilege boundary in [the tunnel-service guide](docs/TUNNEL_SERVICE.md), and the real-hardware procedure in the [Windows package test plan](docs/WINDOWS_TESTING.md). Data-plane deployment is covered by the [Hysteria2](docs/HYSTERIA2.md), [VLESS + REALITY](docs/XRAY_REALITY.md), [HTTPS Stealth](docs/XHTTP_STEALTH.md), [Shadowsocks 2022](docs/SHADOWSOCKS2022.md), [WireGuard TLS](docs/WIREGUARD_TLS.md), and [OpenVPN over stunnel](docs/OPENVPN_STUNNEL.md) guides.

Stable artifact checksums, Android ownership signing, and detached OpenPGP verification are covered by the [release-signing guide](docs/RELEASE_SIGNING.md).

## Server installation details

The one-command installer provisions the official Docker Engine packages when needed, certificates and renewal, all six transports, the hardened HTTPS edge, active-UFW rules, automatic node geography, and a first time-bounded access code. The complete prerequisites, DNS modes, provider ingress examples, location override, certificate lifecycle, operations, and failure recovery are in the [server installation guide](docs/SERVER_INSTALL.md).

## Run with Docker manually

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
docker compose -f compose.yaml -f compose.xray.yaml -f compose.edge.yaml config --quiet
docker compose -f compose.yaml -f compose.xray.yaml -f compose.edge.yaml up --build -d
```

The edge inspects only the TLS ClientHello SNI on TCP `443`: the configured REALITY disguise SNI is passed directly to Xray, while normal Ket HTTPS continues to the certificate-terminating HTTP edge. The REALITY hostname must resolve directly to the server through a DNS-only record; an ordinary Cloudflare Tunnel or orange-cloud HTTP proxy does not forward unmodified VLESS + REALITY traffic. The pinned Xray image is a multi-architecture manifest, so Docker selects the native `linux/amd64` or `linux/arm64` image for the host.

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
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml config --quiet
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml up --build -d
docker compose -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml ps
```

The control, XHTTP, and WireGuard TLS hostnames may remain behind Cloudflare Tunnel, but Hysteria2, REALITY, Shadowsocks, and OpenVPN/stunnel session profiles must advertise a direct server IP or DNS-only hostname. Open only the exact raw-transport ports in both the cloud network policy and host firewall; keep all Cloudflare Tunnel origin ports closed publicly.

Create one client access code on an installed server:

```bash
sudo bash -c '
  set -euo pipefail
  cd /opt/ket
  set -a; . ./.env; set +a
  max_connections=$((KET_MAX_SESSIONS < 5 ? KET_MAX_SESSIONS : 5))
  response_file=$(mktemp)
  trap "rm -f \"$response_file\"" EXIT
  http_status=$(curl --silent --show-error \
    --request POST \
    --header "Authorization: Bearer ${KET_ADMIN_TOKEN}" \
    --header "Content-Type: application/json" \
    --data "{\"label\":\"Personal devices\",\"max_connections\":${max_connections},\"valid_for_minutes\":43200}" \
    --output "$response_file" \
    --write-out "%{http_code}" \
    "http://127.0.0.1:${KET_CONTROL_PORT}/v1/admin/access-grants")
  if [[ $http_status != 201 ]]; then
    jq -r "\"Ket error (\\(.code // \\\"unknown\\\")): \\(.message // \\\"request failed\\\")\"" "$response_file" >&2
    exit 1
  fi
  jq -r "\"Grant ID: \\(.id)\\nAccess code: \\(.access_code)\\nConcurrent connections: \\(.max_connections)\\nValid for: \\(.valid_for_minutes) minutes\\nExpires at: \\(.expires_at_epoch_seconds)\"" "$response_file"
'
```

Create ten separate one-device codes:

```bash
sudo bash -c '
  set -euo pipefail
  cd /opt/ket
  set -a; . ./.env; set +a
  response_file=$(mktemp)
  trap "rm -f \"$response_file\"" EXIT
  http_status=$(curl --silent --show-error \
    --request POST \
    --header "Authorization: Bearer ${KET_ADMIN_TOKEN}" \
    --header "Content-Type: application/json" \
    --data "{\"label_prefix\":\"Client\",\"count\":10,\"max_connections\":1,\"valid_for_minutes\":43200}" \
    --output "$response_file" \
    --write-out "%{http_code}" \
    "http://127.0.0.1:${KET_CONTROL_PORT}/v1/admin/access-grants/batch")
  if [[ $http_status != 201 ]]; then
    jq -r "\"Ket error (\\(.code // \\\"unknown\\\")): \\(.message // \\\"request failed\\\")\"" "$response_file" >&2
    exit 1
  fi
  jq -r ".[] | [.label, .access_code, .valid_for_minutes, .expires_at_epoch_seconds, .id] | @tsv" "$response_file"
'
```

`max_connections` is the number of concurrent sessions allowed by that code and must not exceed the server's `KET_MAX_SESSIONS`. `valid_for_minutes` is required and accepts `1` through `525600` minutes. Ket returns each plaintext code only once and stores only its Argon2 hash. Existing codes cannot be recovered; create a replacement when a code is lost. Use the server URL such as `https://ket.nsa.tools` plus the returned 32-character code in any Ket client. Clients retain the profile and show its remaining access time until this server-enforced lifetime ends.

## Develop

The server, shared contracts, client core, and tunnel service support both `amd64` and `arm64`, including Oracle Ampere hosts. Building the Tauri desktop app additionally requires Tauri's platform webview development packages.

```bash
./scripts/test-all.sh
```

This runs every portable unit and contract suite across the server, shared Rust
code, privileged tunnel service, desktop clients, Android client, installers,
and CI routing. Run `./scripts/test-all.sh all` for formatting, strict lints,
production UI builds, Android lint, and the local real-engine integration
harnesses as well. The complete feature matrix, prerequisites, focused modes,
and physical-test boundary are documented in [the testing guide](docs/TESTING.md).

`/media/n_emperor/Aadhish/gradle-home` is a Gradle user-home cache, not a Gradle executable. Use it with the repository wrapper:

```bash
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home ./gradlew build
```

The Android project is under `apps/ket-android`. It consumes the same control contract as desktop and carries TCP/UDP packets through a local Hysteria2, Xray, Shadowsocks, or WireGuard-over-WSS SOCKS endpoint and the maintained hev TUN bridge. Android uses a platform-specific lifecycle adapter instead of embedding desktop route-management code.

For a local Android build, install Android SDK Platform 34 and Build Tools 34, then run `./packaging/build-android.sh debug`. It auto-detects the SDK used by Abyssal when present; set `KET_ANDROID_SDK` to override it. Gradle installs pinned NDK r27d when needed, while the build downloads and verifies Hysteria 2.10, Xray-core 26.3.27, shadowsocks-rust 1.24.0, wstunnel 10.6.2, OpenVPN for Android 0.7.64, and hev-socks5-tunnel 2.14.0. OpenVPN and Hysteria payloads are included for all four ABIs; Xray and Shadowsocks are included for `arm64-v8a` and `x86_64`; the official Android wstunnel payload is currently arm64-only. Shadowsocks and WireGuard TLS require API 28 or newer, and unsupported devices retain ranked fallback. The generated APK is under `apps/ket-android/app/build/outputs/apk/debug/`. The signer-pinned release procedure is in [the release checklist](docs/RELEASE.md).

Continuous integration is defined in `.github/workflows/ci.yml`: Rust formatting/tests/lints, desktop UI tests/build, native packages, Android debug packaging, and the control-plane container build run when their inputs change. Workflow changes execute the complete matrix, manual runs may select the complete matrix or only the Windows package, and documentation-only changes avoid unnecessary builds.

## Future work

The v1.0 product boundary is complete. Physical release qualification, soak coverage, and future IKEv2 work are intentionally kept in [the future-work register](docs/FUTURE_WORK.md). Ket will continue to use maintained protocol implementations and authenticated encryption; XOR is never treated as security on its own.

## License

Ket is licensed under the [Apache License 2.0](LICENSE). Third-party attributions are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
