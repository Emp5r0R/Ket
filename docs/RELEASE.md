# Release Checklist

## Artifacts

| Artifact | Build path | Current status |
| --- | --- | --- |
| Rust control plane image | `docker build --pull -t ket-control-plane:<tag> .` | Verified on Oracle ARM64 host |
| Server data planes | Compose transport overlays | Hysteria2 and REALITY live-tested on Oracle ARM64; XHTTP/TLS Stealth, Shadowsocks, WireGuard TLS, and OpenVPN/stunnel locally validated with deployment gates pending |
| Linux desktop `.deb` | CI job `linux-package` | Bundles pinned engines including OpenVPN/stunnel; clean install, reinstall, remove, and purge are CI-gated |
| Windows desktop NSIS installer | CI job `windows-package` | Bundles pinned engines, isolated OpenSSL DLL sets, and Wintun; install, reinstall, service, and uninstall are CI-gated |
| Android debug APK | `./packaging/build-android.sh` | Six-transport parser/engine packaging; Hysteria2/REALITY packet flow and recovery exercised on current hardware; OpenVPN/WireGuard TLS/XHTTP/Shadowsocks restricted-network tests pending |
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

For a production server, source `.env`, run `./packaging/validate-env.sh`, and validate the base file plus each enabled overlay with `docker compose config --quiet`. The preflight validates client-visible URL, node identity/location, and enabled transport inputs; `ket-server` then repeats authoritative structured URL and manifest-field validation before it binds a listener. Hysteria2 requires direct UDP reachability; VLESS + REALITY and OpenVPN/stunnel require separate direct raw TCP listeners. XHTTP/TLS Stealth and WireGuard TLS instead require path-specific Cloudflare or compatible HTTP routes to separate loopback-only origins. WireGuard TLS and OpenVPN require a rootful Linux Docker host with `/dev/net/tun`; WireGuard additionally needs kernel WireGuard support.

Container upgrades retain the v1 state volume. The loader accepts older v1 session records that predate scoped data-plane hashes, but those missing credentials remain fail closed until the client creates a current session. Unknown schema versions and structurally inconsistent or oversized state files stop startup instead of discarding grants or guessing a migration. Back up the `ket-state` volume before upgrading; never replace a rejected state file with an empty document as an automated recovery action.

## Signing

The Android debug artifact is only for testing. Production Android, Linux, and Windows artifacts must be signed by the release owner and their checksums published alongside the files. Ket does not store signing keys in this repository.

Android release tasks require `KET_ANDROID_KEYSTORE`, `KET_ANDROID_KEYSTORE_PASSWORD`, `KET_ANDROID_KEY_ALIAS`, and `KET_ANDROID_KEY_PASSWORD`. `packaging/build-android.sh release` additionally requires `KET_ANDROID_CERT_SHA256` and refuses an APK whose signer differs from that pinned certificate. Set `KET_ANDROID_VERSION_CODE` and `KET_ANDROID_VERSION_NAME` for each release; they default to the development values `1` and `0.1.0` only when omitted. The CI Android job builds the release variant with an ephemeral key and validates its fingerprint, then deliberately uploads only the debug APK.

The maintained server and Linux/Windows data planes are Hysteria2, Xray-core VLESS + REALITY, Xray-core VLESS + XHTTP/TLS Stealth, shadowsocks-rust Shadowsocks 2022, WireGuard over wstunnel WebSocket/TLS, and OpenVPN over stunnel TLS. SOCKS-backed desktop transports share a locally tested full-route bridge; OpenVPN owns a separately supervised native TUN. Android implements all six through platform-owned routing, including a private OpenVPN management/TUN bridge and certificate-pinned stunnel-compatible TLS carrier. Android Hysteria2 and REALITY packet flow, startup fallback, engine-exit recovery, bidirectional Wi-Fi/cellular recovery, cancellation, and disconnect have been exercised on a physical current arm64 device; OpenVPN, WireGuard TLS, XHTTP, and Shadowsocks currently have strict local parsing, engine, and package validation only. Fail-closed handover, restricted-network traffic, Doze-exit renewal, graceful permission-revoke, always-on/reboot, API 26, connected-state DNS-leak, lockdown, and owner-signed release tests remain. WireGuard and OpenVPN server lifecycles must be exercised on a capable Linux deployment host. IKEv2 and XOR remain contract-level extension points rather than implemented adapters.

## Multi-architecture image

Publish one manifest for both supported server architectures with an authenticated registry login:

```bash
./packaging/build-multiarch.sh ghcr.io/your-org/ket-control-plane:<tag>
```

The script publishes `linux/amd64` and `linux/arm64`. Docker selects the matching image on the host, so the same tag works on Oracle Ampere and AMD64 servers. The command requires Buildx and registry push permission; it does not alter the local Docker daemon image store.
