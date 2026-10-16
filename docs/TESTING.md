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
| `./scripts/test-all.sh integration` | Docker edge probe-resistance harness plus real local OpenVPN/stunnel, Shadowsocks 2022, and XHTTP/TLS engine lifecycles |
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
| Invite capsule | Canonical CBOR (shortest forms, re-encoding match), every-bit tamper rejection, semantic rejections behind valid signatures, locator safety rules, text form canonicality, expiry after signature, enrollment signatures bound to node/nonce/exact body; Rust-generated vectors (`crates/ket-invite/tests/vectors/invite-v1.json`) that fail on drift and are replayed by the Android JVM tests through the JNI library (`crates/ket-invite-android`, built for the host) with identical error codes; the shared format crate (`crates/invite-capsule`) tests every rule through two example profiles, including cross-application rejection and the manual text form; server issuance for single and batch grants, signed enrollment only with a valid nonce, pinned desktop/Android enrollment over real loopback HTTP; desktop invite-first entry, local verification, native QR image decoding (`crates/ket-qr`: bounded PNG/JPEG normalization, terminal-gap, inverted, cropped, and noisy captures), QR file/paste handling, node-key persistence and pinned restore; Android invite reader on shared vectors, ZXing QR round trips (luminance, ARGB, transparency), bounded image reads, stride-aware camera planes, and pinned profile persistence/re-enrollment; `packaging/test-create-invite.sh` against a stub admin API |
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
| Android client | Strict node/session parsing, durable authenticated credentials, all maintained transport parsers/configs plus Hysteria XOR mode, ranking/recovery, route/DNS ownership, network transitions, runtime subscriptions, pending launch ownership, diagnostics, map geometry, and UI status copy |
| Public edge | Renderer validation and injection rejection, exact transport routing, operational routes absent from the public vhost, masked versus pass-through status lists (`packaging/test-render-edge.sh`); pinned nginx image probed as a scanner and as a client against stub upstreams, byte-identical decoy responses (`packaging/verify-edge-proxy.sh`, needs Docker) |
| Server installation and CI | Installer command line and plan output, installer libraries (Cloudflare ranges, mode detection with stubbed DNS, OS-family and Docker repository mapping across Debian/Ubuntu/Mint/Rocky/RHEL/Fedora/Amazon/openSUSE/Arch/Alpine, renewal units, secret formats), the `install.sh` bootstrap against a stub GitHub (latest-release redirect, legacy releases, failures, cleanup), Compose overlay selection, real provisioning with Docker-generated keys (`verify-installer-provisioning.sh`, integration), certificate trust rotation, every transport preflight with negative cases, shell syntax, and conservative affected-job routing |
| Android release packaging | `packaging/test-validate-android-apk.sh`: universal APKs need every ABI's engines and the Rust invite library, an arm64 or other single-ABI split needs exactly its own ABI and nothing else, unknown ABIs and missing engines are rejected, and a pinned signer must match; CI builds debug and release splits and validates `arm64-v8a` and `universal` of each |
| Release qualification | `scripts/release-preflight.sh` runs on real systems before a tag (see [RELEASE.md](RELEASE.md#release-candidate)): version agreement, CI for the commit, the full suite, the real installer and bootstrap, and on the canary `ket doctor`, public-name exposure of every operational endpoint, and invite enrollment through `ket-client-core`'s `invite_enroll_check` example; `scripts/test-release-preflight.sh` covers its version and masking decisions |
| Update lifecycle | `ket-update`: strict versions and `Ket-Client` headers (malformed means legacy), manifest validation, the embedded manifest ending at the workspace version, client ratings (optional to required, security grace deadline, platform filters, operator minimum, `warn` enforcement), and the server-update rule replayed from `server-updates.json`; server routes attach advice, refuse required updates with `426` only after authentication, roll the session back, and refuse renewal; the edge passes `426` and `Ket-Client` through (`verify-edge-proxy.sh`); desktop client core sends the header, keeps advice in snapshots, rejects unsafe download pages, and ends the session on `426`; desktop banner and settings; Android parser, refusal handling, launch-spec and stored-manifest round trip, and notices |
| Server `ket` command | `packaging/test-ket-cli.sh` with stubbed Docker and systemd: the jq update rule on the shared vectors, `KET_AUTO_UPDATE` policies, `.env` editing, prompts and the invite wizard, node fingerprints on the invite vectors, timers and the command link (including v1.0.1 host-certbot installs), service-state parsing, Cloudflare Tunnel routes that bypass the edge (doctor), the installation's control port, update decisions, a full apply with configuration carry-over and migration, automatic restore of an unready release, rejected configurations, rollback both ways, published-image versus local build, uninstall with confirmation, final backup, and purge; `test-bootstrap.sh` covers `install.sh update` and `uninstall` |

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
