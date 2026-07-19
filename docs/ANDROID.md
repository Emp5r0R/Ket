# Android data plane

Ket Android consumes the versioned control API directly and implements platform-owned Hysteria2 and VLESS + REALITY adapters. It does not run the desktop broker or attempt desktop-style privileged route changes.

## Packet path

1. The UI obtains Android VPN permission before exchanging the 32-character access code.
2. The control response is parsed fail-closed and ranked by server priority. Android accepts strict Hysteria2 UDP profiles and VLESS + REALITY profiles with Vision, a supported TLS fingerprint, and validated public-key material.
3. The service attempts each supported transport in rank order. An unavailable 64-bit Xray payload, failed REALITY handshake, or other retryable startup failure falls through to Hysteria2 without weakening either profile. Successful latency and failures feed a bounded exponential-cooldown history.
4. Hysteria starts in SOCKS5 TCP/UDP mode and sends its outbound QUIC descriptor over an abstract Unix socket. Ket receives it through `SCM_RIGHTS`, calls `VpnService.protect`, closes the duplicate, and acknowledges the engine.
5. Xray starts with a loopback SOCKS inbound, validates its generated configuration, and proves the REALITY path with a SOCKS CONNECT before Android installs full routes. The exact resolved server address is excluded from the VPN to prevent recursion.
6. After the selected SOCKS endpoint is ready, Ket establishes IPv4 and IPv6 Android routes and passes the TUN descriptor to hev-socks5-tunnel.
7. The foreground service owns renewal, traffic sampling, process and routed-path health, failure reporting, and session release. HTTPS lease renewal traverses the active VPN and is independent of local bridge statistics. Two consecutive failures trigger transport recovery; shutdown remains deferred until five failures leave room for that recovery window.
8. If an established engine exits or its routed control path fails, Ket retains the lease, cools the failed transport, tears down the old bridge and TUN, and attempts the ranked alternatives. Each successful replacement gets a newly established TUN with either protected Hysteria sockets or the exact Xray server-route exclusion. Recovery is bounded to three rounds and does not itself release the session.

Transport configurations are mode `0600` under `noBackupFilesDir` and are deleted after readiness. The hev configuration contains only loopback plumbing and is removed at shutdown. Credentials are never written to logs or exposed through state snapshots.

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

The debug APK is build evidence, not a production release. CI generates a short-lived signing identity to prove that the release variant, native payloads, and certificate check work together, but it neither uploads that release APK nor establishes a production signer. Unit tests cover strict parsing for both transports, IPv4/IPv6 route exclusion, deterministic recovery ranking/cooldown, cooperative startup cancellation, and process-output shutdown races.

On 2026-07-19, the arm64 payload was exercised on a physical Android 16/API 36 device against the dual-transport Oracle ARM deployment. Both Hysteria2 and VLESS + REALITY established protected routes and reported nonzero bidirectional traffic. The device also proved automatic startup fallback, recovery after forcibly terminating the active Xray child while retaining its lease, cancellation with both data planes unreachable, lease release, and repeated crash-free disconnects. Temporary server firewall rules used for the unreachable-path test were removed and readiness was rechecked afterward.

Repeat this matrix on a physical API 26 device. Network switching, Doze, VPN permission revoke, explicit DNS leak checks, and signed-release installation remain physical-device release gates. Release signing material is intentionally not stored in this repository.
