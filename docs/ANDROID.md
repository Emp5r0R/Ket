# Android data plane

Ket Android consumes the versioned control API directly and implements a platform-owned Hysteria2 adapter. It does not run the desktop broker or attempt privileged route changes.

## Packet path

1. The UI obtains Android VPN permission before exchanging the 32-character access code.
2. The control response is parsed fail-closed. Android accepts only a Hysteria2 UDP profile with strict TLS SNI, known obfuscation options, and a separate data-plane credential.
3. The service resolves the server before installing VPN routes and starts the pinned Hysteria executable in SOCKS5 TCP/UDP mode.
4. Hysteria sends its outbound QUIC descriptor over an abstract Unix socket. Ket receives it through `SCM_RIGHTS`, calls `VpnService.protect`, closes the duplicate, and acknowledges the engine.
5. After Hysteria reports a server connection and its loopback SOCKS port accepts connections, Ket establishes IPv4 and IPv6 Android routes and passes the TUN descriptor to hev-socks5-tunnel.
6. The foreground service owns renewal, traffic sampling, process health, failure reporting, and session release. Closing the Activity does not stop the VPN.

The Hysteria configuration is mode `0600` under `noBackupFilesDir` and is deleted after readiness. The hev configuration contains only loopback plumbing and is removed at shutdown. Credentials are never written to logs or exposed through state snapshots.

## Native supply chain

`packaging/prepare-android-engines.sh` downloads checksum-pinned Hysteria 2.10 executables for `armeabi-v7a`, `arm64-v8a`, `x86`, and `x86_64`. It also verifies and expands the complete hev-socks5-tunnel 2.14.0 source release. NDK r27d compiles hev and Ket's small API-26-compatible FD receiver for every ABI.

Generated third-party files live under `app/build/generated/ket-engines` and are not tracked. The APK validator requires Hysteria, hev, and the Ket JNI shim for every ABI before CI uploads the artifact.

## Verification

```bash
./packaging/build-android.sh
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home \
  apps/ket-android/gradlew --no-daemon testDebugUnitTest assembleDebug lintDebug
./packaging/validate-android-apk.sh \
  apps/ket-android/app/build/outputs/apk/debug/app-debug.apk
```

The debug APK is build evidence, not a production release. The arm64 payload has been installed on a current Android 16 device: cold start, VPN consent, loopback enrollment, foreground-service startup, strict DNS failure reporting, lease release, and direct execution of the pinned Hysteria 2.10 binary were verified. End-to-end TCP, UDP, and DNS packet flow still requires a reachable Hysteria deployment with a trusted certificate; network switching, Doze, revoke, and disconnect also remain release gates. Repeat the matrix on a physical API 26 device. Release signing material is intentionally not stored in this repository.
