# Release Checklist

## Artifacts

| Artifact | Build path | Current status |
| --- | --- | --- |
| Rust control plane image | `docker build --pull -t ket-control-plane:<tag> .` | Verified on Oracle ARM64 host |
| Server data planes | Dual Compose overlays | Live-tested VLESS + REALITY over TCP and Salamander-obfuscated Hysteria2 over UDP on Oracle ARM64 |
| Linux desktop `.deb` | CI job `linux-package` | Bundles pinned engines; clean install, reinstall, remove, and purge are CI-gated |
| Windows desktop NSIS installer | CI job `windows-package` | Bundles pinned engines and Wintun; install, reinstall, service, and uninstall are CI-gated |
| Android debug APK | `./packaging/build-android.sh` | Multi-ABI Hysteria and 64-bit Xray payloads built and validated; physical dual-transport packet flow pending |

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

For a production server, source `.env`, run `./packaging/validate-env.sh`, and validate the base file plus each enabled overlay with `docker compose config --quiet`. Hysteria2 requires direct UDP reachability and VLESS + REALITY requires direct raw TCP reachability; ordinary Cloudflare HTTP proxying or a Cloudflare Tunnel does not carry either unmodified data plane.

## Signing

The Android debug artifact is only for testing. Production Android, Linux, and Windows artifacts must be signed by the release owner and their checksums published alongside the files. Ket does not store signing keys in this repository.

The maintained server and client data planes are Hysteria2 and Xray-core VLESS + REALITY. Desktop REALITY has a full-route Docker integration test; Android REALITY remains pre-release until physical-device packet flow and fallback are verified. Other protocol identifiers remain contract-level extension points.

## Multi-architecture image

Publish one manifest for both supported server architectures with an authenticated registry login:

```bash
./packaging/build-multiarch.sh ghcr.io/your-org/ket-control-plane:<tag>
```

The script publishes `linux/amd64` and `linux/arm64`. Docker selects the matching image on the host, so the same tag works on Oracle Ampere and AMD64 servers. The command requires Buildx and registry push permission; it does not alter the local Docker daemon image store.
