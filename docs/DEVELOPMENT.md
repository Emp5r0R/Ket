# Development

The server, shared contracts, client core, and tunnel service support both `amd64` and `arm64`, including Oracle Ampere hosts. Building the Tauri desktop app additionally requires Tauri's platform webview development packages.

```bash
./scripts/test-all.sh
```

This runs every portable unit and contract suite across the server, shared Rust
code, privileged tunnel service, desktop clients, Android client, installers,
and CI routing. Run `./scripts/test-all.sh all` for formatting, strict lints,
production UI builds, Android lint, and the local real-engine integration
harnesses as well. The complete feature matrix, prerequisites, focused modes,
and physical-test boundary are documented in [the testing guide](TESTING.md).

`/media/n_emperor/Aadhish/gradle-home` is a Gradle user-home cache, not a Gradle executable. Use it with the repository wrapper:

```bash
GRADLE_USER_HOME=/media/n_emperor/Aadhish/gradle-home ./gradlew build
```

The Android project is under `apps/ket-android`. It consumes the same control contract as desktop and carries TCP/UDP packets through a local Hysteria2, Xray, Shadowsocks, or WireGuard-over-WSS SOCKS endpoint and the maintained hev TUN bridge. Android uses a platform-specific lifecycle adapter instead of embedding desktop route-management code.

For a local Android build, install Android SDK Platform 34 and Build Tools 34, then run `./packaging/build-android.sh debug`. It auto-detects the SDK used by Abyssal when present; set `KET_ANDROID_SDK` to override it. Gradle installs pinned NDK r27d when needed, while the build downloads and verifies Hysteria 2.10, Xray-core 26.3.27, shadowsocks-rust 1.24.0, wstunnel 10.6.2, OpenVPN for Android 0.7.64, and hev-socks5-tunnel 2.14.0. OpenVPN and Hysteria payloads are included for all four ABIs; Xray and Shadowsocks are included for `arm64-v8a` and `x86_64`; the official Android wstunnel payload is currently arm64-only. Shadowsocks and WireGuard TLS require API 28 or newer, and unsupported devices retain ranked fallback. The generated APK is under `apps/ket-android/app/build/outputs/apk/debug/`. Invites are verified by Rust code built from `crates/ket-invite-android` with the same NDK, so Android builds also need `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`. The signer-pinned release procedure is in [the release checklist](RELEASE.md).

Continuous integration is defined in `.github/workflows/ci.yml`: Rust formatting/tests/lints, desktop UI tests/build, native packages, Android debug packaging, and the control-plane container build run when their inputs change. Workflow changes execute the complete matrix, manual runs may select the complete matrix or only the Windows package, and documentation-only changes avoid unnecessary builds.
