# Android data plane

Ket Android consumes the versioned control API directly and implements platform-owned Hysteria2 and VLESS + REALITY adapters. It does not run the desktop broker or attempt desktop-style privileged route changes.

## Packet path

1. The UI obtains Android VPN permission before exchanging the 32-character access code.
2. The control response is parsed fail-closed and ranked by server priority. Android accepts strict Hysteria2 UDP profiles and VLESS + REALITY profiles with Vision, a supported TLS fingerprint, and validated public-key material.
3. A successful manual enrollment atomically persists the normalized control endpoint, access grant, and complete current session manifest in an authenticated encrypted record. An app launch is explicitly flagged; an unflagged system launch restores the current lease for always-on VPN. Ket renews a saved session before use, replaces it only after confirmed authorization loss, and retains it optimistically when a restricted network blocks the control endpoint.
4. The service attempts each supported transport in rank order. An unavailable 64-bit Xray payload, failed REALITY handshake, or other retryable startup failure falls through to Hysteria2 without weakening either profile. Successful latency and failures feed a bounded exponential-cooldown history.
5. Hysteria starts in SOCKS5 TCP/UDP mode and sends its outbound QUIC descriptor over an abstract Unix socket. Ket receives it through `SCM_RIGHTS`, calls `VpnService.protect`, closes the duplicate, and acknowledges the engine.
6. Before installing a VPN route, Ket resolves each advertised data-plane endpoint and pins the selected engine to a numeric address. Xray starts with a loopback SOCKS inbound, validates its generated configuration, and proves the REALITY path with a SOCKS CONNECT. The pinned endpoint addresses are excluded from the VPN to prevent recursion during startup and ranked fallback.
7. After the selected SOCKS endpoint is ready, Ket selects redundant explicit [Cloudflare resolver addresses](https://developers.cloudflare.com/1.1.1.1/ip-addresses/) for IPv4 and IPv6, removes any address that overlaps a transport exclusion, and fails closed if either family has no remaining resolver. It registers those DNS servers with `VpnService.Builder`, never enables `allowBypass`, installs full routes, and passes the TUN descriptor to hev-socks5-tunnel with SOCKS UDP enabled. API 26-32 receive an exact CIDR complement for every excluded address; API 33 and newer use platform exclusion routes.
8. The foreground service owns renewal, traffic sampling, process and routed-path health, failure reporting, and session release. It also observes only non-VPN internet networks, debounces Wi-Fi/mobile callback bursts, and triggers recovery after the available underlying network set changes or returns from an outage. HTTPS lease renewal traverses the active VPN and is independent of local bridge statistics. Ket ignores expected network suspension while Android reports Doze, then serializes an immediate renewal and status refresh when idle mode ends. Authorization loss stops immediately; ordinary failures retain the bounded recovery window.
9. If an established engine exits, its routed control path fails, or the underlying network changes, Ket retains the lease and the last established TUN while it stops the old bridge and engine. That TUN keeps application and DNS traffic fail closed while ranked alternatives start. Android establishes the replacement TUN before Ket releases the previous descriptor, then Ket attaches the bridge; a failed bridge attach leaves the newest TUN installed as the next guard. Recovery is bounded to three rounds and does not itself release the session.

Transport configurations are mode `0600` under `noBackupFilesDir` and are deleted after readiness. The hev configuration contains only loopback plumbing and is removed at shutdown. Credentials are never written to logs or exposed through state snapshots.

The durable record also lives under `noBackupFilesDir` and is written through `AtomicFile`. Its contents are sealed with AES-256-GCM and a non-exportable [Android Keystore](https://developer.android.com/privacy-and-security/keystore) key; credentials cannot migrate through Android backup without that device-local key. The first manual connection must succeed before enabling [always-on VPN](https://developer.android.com/develop/connectivity/vpn#always-on). Android can then start the foreground service after process death, reboot, or app upgrade, while system lockdown blocks traffic before Ket establishes its full route. The packaged manifest advertises always-on support, but the reboot and lockdown lifecycle remains a physical-device release gate.

## Native supply chain

`packaging/prepare-android-engines.sh` downloads checksum-pinned Hysteria 2.10 executables for `armeabi-v7a`, `arm64-v8a`, `x86`, and `x86_64`, plus Xray-core 26.3.27 for `arm64-v8a` and `x86_64`. It also verifies and expands the complete hev-socks5-tunnel 2.14.0 source release. NDK r27d compiles hev and Ket's small API-26-compatible FD receiver for every ABI. Official Xray Android releases do not provide the two 32-bit payloads, so those APK splits retain Hysteria2 fallback only.

Generated third-party files live under `app/build/generated/ket-engines` and are not tracked. The APK validator requires Hysteria, hev, and the Ket JNI shim for every ABI and Xray for both supported 64-bit ABIs before CI uploads the artifact.

## Verification

```bash
./packaging/build-android.sh debug
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home \
  apps/ket-android/gradlew --no-daemon testDebugUnitTest assembleDebug lintDebug
./packaging/validate-android-apk.sh \
  apps/ket-android/app/build/outputs/apk/debug/app-debug.apk
```

`./packaging/build-android.sh` defaults to `debug`; the explicit argument avoids confusing a test build with a release. A release build requires all signing variables and pins the resulting APK to the expected certificate:

```bash
export KET_ANDROID_KEYSTORE=/secure/path/ket-release.p12
export KET_ANDROID_KEYSTORE_PASSWORD='<keystore password>'
export KET_ANDROID_KEY_ALIAS=ket-release
export KET_ANDROID_KEY_PASSWORD='<key password>'
export KET_ANDROID_CERT_SHA256='<64-hex-digit certificate SHA-256>'
export KET_ANDROID_VERSION_CODE=2
export KET_ANDROID_VERSION_NAME=0.2.0
./packaging/build-android.sh release
```

Obtain the expected fingerprint with `keytool -list -v -keystore "$KET_ANDROID_KEYSTORE" -alias "$KET_ANDROID_KEY_ALIAS"`; let `keytool` prompt for the password. Gradle rejects every release task when the signing environment is missing or partial. The wrapper then verifies the APK signature and requires its signer certificate to match `KET_ANDROID_CERT_SHA256` before reporting success.

The debug APK is build evidence, not a production release. CI generates a short-lived signing identity to prove that the release variant, native payloads, and certificate check work together, but it neither uploads that release APK nor establishes a production signer. Unit tests cover strict parsing for both transports, authenticated credential-envelope tamper detection, durable session resume/re-enrollment decisions, orphan-session cleanup, collision-safe dual-stack VPN DNS selection, SOCKS UDP forwarding, IPv4/IPv6 route inclusion and exclusion, fail-closed replacement-route ownership, Doze refresh gating, terminal authorization loss, deterministic recovery ranking/cooldown, underlying-network transition decisions, cooperative startup cancellation, and process-output shutdown races.

On 2026-07-19, the arm64 payload was exercised on a physical Android 16/API 36 device against the dual-transport Oracle ARM deployment. Both Hysteria2 and VLESS + REALITY established protected routes and reported nonzero bidirectional traffic. The device also proved automatic startup fallback, recovery after forcibly terminating the active Xray child while retaining its lease, cancellation with both data planes unreachable, lease release, and repeated crash-free disconnects. Temporary server firewall rules used for the unreachable-path test were removed and readiness was rechecked afterward.

On 2026-07-20, USB-controlled testing switched the same connected device from Wi-Fi to cellular and back. Ket retained its lease, changed the Android underlying network in both directions, recovered from VLESS + REALITY to Hysteria2 and back to VLESS + REALITY, and returned to a validated full-route VPN with the configured VPN DNS servers. Sampling also caught the VPN interface disappearing during the old cleanup sequence. The recovery implementation now retains an established TUN as a fail-closed guard and replaces it only after Android establishes the next interface; local unit, build, lint, signature, and APK payload checks pass, but this correction still requires a physical repeat.

Repeat the corrected switch test on the API 36 device and the complete matrix on a physical API 26 device. The implemented Doze-exit renewal, graceful VPN-permission revoke, process/reboot restoration, always-on/lockdown lifecycle, explicit connected-state DNS leak behavior, and owner-signed release installation remain physical-device release gates. Release signing material is intentionally not stored in this repository.
