# Testing Ket

Ket has one repository-level test entry point:

```bash
./scripts/test-all.sh
```

The default `unit` mode runs all portable Rust, desktop-native, desktop UI,
Android JVM, installer, certificate, environment, and CI-routing tests. Tests
are discovered through each workspace or test runner rather than named one at a
time, so a new Cargo workspace member, Vitest file, or Android debug unit test is
included automatically.

## Modes

| Command | Scope |
| --- | --- |
| `./scripts/test-all.sh` | Every portable unit and contract suite |
| `./scripts/test-all.sh quality` | Rust formatting and Clippy, desktop production build, Android lint, patch integrity |
| `./scripts/test-all.sh integration` | Real local OpenVPN/stunnel, Shadowsocks 2022, and XHTTP/TLS engine lifecycles |
| `./scripts/test-all.sh all` | Unit, quality, and local engine integration gates |
| `./scripts/test-all.sh rust` | Rust tests plus formatting and strict Clippy |
| `./scripts/test-all.sh desktop-ui` | Vitest interactions plus the TypeScript/Vite production build |
| `./scripts/test-all.sh desktop-native` | Optimized Tauri native tests |
| `./scripts/test-all.sh android` | Android JVM tests plus lint |
| `./scripts/test-all.sh server` | Every maintained Bash syntax and server/CI contract test |

The portable suite does not install dependencies. Run `npm ci --prefix
apps/ket-desktop`, install Rust 1.88, Java 17, and configure Android SDK Platform
34 before the first run. Android SDK discovery follows the Gradle project's
`local.properties`, `ANDROID_HOME`, and `ANDROID_SDK_ROOT` conventions.

Linux desktop-native tests also need the Tauri development libraries used by
CI. On Ubuntu or Debian:

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libxdo-dev patchelf
```

## Feature Coverage

| Product boundary | Maintained evidence |
| --- | --- |
| Shared control contract | Exact protocol/network/health wire names, request/status lifetimes, optional-field compatibility, manifest round trips, and debug redaction |
| Secret handling | Access/session token shapes, Argon2 policy, scoped token hashes, broker token permissions, HMAC proof tampering, and secret-redacted diagnostics |
| Control server | Configuration bounds, grant/session API authorization and lifecycle, persistence validation and restart, capacity under concurrency, renewal/release/revocation, and bounded errors |
| Server data plane | Deterministic credentials, counters, resource allocation, reconciliation/revocation, runtime renderers, all six environment profiles, and system telemetry parsing |
| Client controller | Enrollment, strict server response validation, Automatic fallback, explicit protocol locking, ranking/cooldown, DNS cache recovery, cancellation, reconnect, renewal, route-health gating, telemetry, and cleanup |
| Hysteria2 | Strict client/server renderers, all obfuscation modes, process supervision, private atomic configuration, and local protocol lifecycle |
| VLESS + REALITY | Strict REALITY fields and fingerprints, Xray config validation, scoped users/stats, engine acceptance, and optional routed egress test |
| HTTPS Stealth | Strict XHTTP/TLS path and SNI, CDN-safe server profile, engine traffic/counters/release/revocation harness |
| Shadowsocks 2022 | SIP022 method/key bounds, TCP/UDP client config, manager parsing, engine traffic/key enforcement/counters/revocation harness |
| WireGuard TLS | Key/address/lease bounds, wstunnel TLS path, Xray userspace WireGuard config, server peer counters and revocation contracts |
| OpenVPN TLS | Certificate and `tls-crypt` bounds, hardened configs, management/TUN settings, scoped auth, counters, release/revocation, and real nested handshake harness |
| Privileged tunnel service | Framing limits, authentication, loopback-only broker policy, lease expiry, heartbeat/status/stop behavior, DNS restoration, and desktop exchange |
| Desktop client | Saved/expired profiles, reconnect policy, theme, every view/component, every lifecycle state, protocol guides, availability, maps, telemetry, diagnostics copying, engine warnings, and Tauri protocol commands |
| Android client | Strict node/session parsing, durable authenticated credentials, all six transport parsers/configs, ranking/recovery, route/DNS ownership, network transitions, runtime subscriptions, pending launch ownership, diagnostics, map geometry, and UI status copy |
| Server installation and CI | Installer arguments/templates, certificate trust rotation, every transport preflight with negative cases, shell syntax, and conservative affected-job routing |

## Required Test Design

New behavior must be tested at its owning layer. Pure parsing, validation,
ranking, lifecycle, and rendering logic belongs in deterministic unit tests.
External engines require a contract test plus an integration harness. A bug fix
must include a regression test that fails for the original behavior. Tests must
not log access codes, session tokens, private keys, passwords, or full untrusted
server errors.

A new client or protocol must update this matrix and be added to the root runner
before it is considered implemented. Unknown paths deliberately select every CI
job, preventing a new component from bypassing verification until its ownership
rules are defined.

## Non-Unit Release Gates

Unit tests cannot prove kernel TUN behavior, public Internet egress, censorship
resistance, Wi-Fi/mobile handover, Doze, reboot, always-on/lockdown mode, owner
signing, or native Windows installation. Those remain explicit physical and
package gates in [the release checklist](RELEASE.md), [Android guide](ANDROID.md),
[Windows test plan](WINDOWS_TESTING.md), and [future-work register](FUTURE_WORK.md).
They must not be described as passing based only on this script.
