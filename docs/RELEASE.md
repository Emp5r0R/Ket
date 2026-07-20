# Release Checklist

## Artifacts

| Artifact | Build path | Current status |
| --- | --- | --- |
| Rust control plane image | `docker build --pull -t ket-control-plane:<tag> .` | Verified on Oracle ARM64 host |
| Server data planes | Dual Compose overlays | Live-tested VLESS + REALITY over TCP and Salamander-obfuscated Hysteria2 over UDP on Oracle ARM64 |
| Linux desktop `.deb` | CI job `linux-package` | Bundles pinned engines; clean install, reinstall, remove, and purge are CI-gated |
| Windows desktop NSIS installer | CI job `windows-package` | Bundles pinned engines and Wintun; install, reinstall, service, and uninstall are CI-gated |
| Android debug APK | `./packaging/build-android.sh` | Real server map and node telemetry parity implemented; multi-ABI Hysteria and 64-bit Xray payloads validated; dual-transport packet flow, fallback, recovery, Wi-Fi/cellular switching, cancellation, and disconnect exercised on current arm64 hardware; fail-closed handover retest pending |
| Android release APK | `./packaging/build-android.sh release` | Fail-closed signing and signer pinning are CI-gated with a disposable identity; owner-signed installation remains pending |

## Required checks

Run these before publishing a release:

```bash
cargo fmt --all -- --check
cargo test --locked --workspace --exclude ket-desktop
cargo clippy --locked --workspace --exclude ket-desktop --all-targets --all-features -- -D warnings
cargo test --locked --release -p ket-desktop --lib
npm --prefix apps/ket-desktop test -- --run
npm --prefix apps/ket-desktop run build
sudo env KET_PACKAGE_TEST_ALLOW_HOST_MUTATION=1 KET_PACKAGE_TEST_USER="$USER" ./packaging/verify-linux-deb.sh target/release/bundle/deb/*.deb
./packaging/prepare-android-engines.sh apps/ket-android/app
(cd apps/ket-android && ./gradlew --no-daemon testDebugUnitTest assembleDebug lintDebug)
./packaging/validate-android-apk.sh apps/ket-android/app/build/outputs/apk/debug/app-debug.apk
```

Run the Windows NSIS lifecycle only from an elevated shell on a disposable test host:

```powershell
$env:KET_PACKAGE_TEST_ALLOW_HOST_MUTATION = "1"
$installer = Get-ChildItem target/release/bundle/nsis/*.exe -File
./packaging/verify-windows-nsis.ps1 -Installer $installer.FullName
```

For a production server, source `.env`, run `./packaging/validate-env.sh`, and validate the base file plus each enabled overlay with `docker compose config --quiet`. The preflight validates client-visible URL, node identity/location, and enabled transport inputs; `ket-server` then repeats authoritative structured URL and manifest-field validation before it binds a listener. Hysteria2 requires direct UDP reachability and VLESS + REALITY requires direct raw TCP reachability; ordinary Cloudflare HTTP proxying or a Cloudflare Tunnel does not carry either unmodified data plane.

Container upgrades retain the v1 state volume. The loader accepts older v1 session records that predate scoped data-plane hashes, but those missing credentials remain fail closed until the client creates a current session. Unknown schema versions and structurally inconsistent or oversized state files stop startup instead of discarding grants or guessing a migration. Back up the `ket-state` volume before upgrading; never replace a rejected state file with an empty document as an automated recovery action.

## Signing

The Android debug artifact is only for testing. Production Android, Linux, and Windows artifacts must be signed by the release owner and their checksums published alongside the files. Ket does not store signing keys in this repository.

Android release tasks require `KET_ANDROID_KEYSTORE`, `KET_ANDROID_KEYSTORE_PASSWORD`, `KET_ANDROID_KEY_ALIAS`, and `KET_ANDROID_KEY_PASSWORD`. `packaging/build-android.sh release` additionally requires `KET_ANDROID_CERT_SHA256` and refuses an APK whose signer differs from that pinned certificate. Set `KET_ANDROID_VERSION_CODE` and `KET_ANDROID_VERSION_NAME` for each release; they default to the development values `1` and `0.1.0` only when omitted. The CI Android job builds the release variant with an ephemeral key and validates its fingerprint, then deliberately uploads only the debug APK.

The maintained server and client data planes are Hysteria2 and Xray-core VLESS + REALITY. Both desktop transports share a locally tested full-route bridge that enforces virtual DNS, IPv4/IPv6 capture, server-IP bypass, paired process supervision, and cleanup; REALITY also has a full-route Docker integration test. Android Hysteria2 and REALITY packet flow, startup fallback, engine-exit recovery, bidirectional Wi-Fi/cellular recovery, cancellation, and disconnect have been exercised on a physical current arm64 device. Fail-closed handover, Doze-exit renewal, graceful permission-revoke, and always-on/reboot corrections need physical API 36 repeats; Android API 26, connected-state DNS-leak, lockdown, and owner-signed release testing also remain. Always-on support is enabled only after implementing a Keystore-sealed durable enrollment/session record and boot-safe session resolution. Other protocol identifiers remain contract-level extension points.

## Multi-architecture image

Publish one manifest for both supported server architectures with an authenticated registry login:

```bash
./packaging/build-multiarch.sh ghcr.io/your-org/ket-control-plane:<tag>
```

The script publishes `linux/amd64` and `linux/arm64`. Docker selects the matching image on the host, so the same tag works on Oracle Ampere and AMD64 servers. The command requires Buildx and registry push permission; it does not alter the local Docker daemon image store.
