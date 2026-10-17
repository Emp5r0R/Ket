# Release Checklist

The v1.0 implementation scope is complete as of 2026-07-28. This checklist separates repeatable source/package evidence from physical hardware and network qualification; the latter remains tracked in [future work](FUTURE_WORK.md) and does not reduce implemented protocol support.

## Artifacts

| Artifact | Build path | Current status |
| --- | --- | --- |
| Rust control plane image | `docker build --pull -t ket-control-plane:<tag> .` | Verified on Oracle ARM64 host |
| Server data planes | Compose transport overlays | All six overlays deployed on Oracle ARM64; REALITY, XHTTP/TLS Stealth, and Shadowsocks passed owner-signed Android restricted-network routes; Hysteria2, WireGuard TLS, and OpenVPN Android packet flow remains pending |
| Linux desktop `.deb` | CI job `linux-package` | Bundles pinned engines including OpenVPN/stunnel; clean install, reinstall, remove, and purge are CI-gated |
| Windows desktop NSIS installer | CI job `windows-package` | Bundles pinned engines, isolated OpenSSL DLL sets, and Wintun; normal CI gates the lifecycle, while the manual build-only run is followed by the [real-Windows test plan](WINDOWS_TESTING.md) |
| Android debug APKs | `./packaging/build-android.sh` | All maintained data planes plus Hysteria XOR parser/engine packaging; REALITY, XHTTP/TLS Stealth, and Shadowsocks routed traffic exercised on current hardware; Hysteria2/OpenVPN/WireGuard TLS gates pending |
| Android release APKs (`android-arm64`, `android-universal`) | `./packaging/build-android.sh release --out <dir>` | Fail-closed signing and signer pinning verified; owner-signed installation and restricted-network egress passed on current arm64 hardware |

## Required checks

Run these before publishing a release:

```bash
./scripts/test-all.sh all
sudo env KET_PACKAGE_TEST_ALLOW_HOST_MUTATION=1 KET_PACKAGE_TEST_USER="$USER" ./packaging/verify-linux-deb.sh target/release/bundle/deb/*.deb
./packaging/prepare-android-engines.sh apps/ket-android/app
(cd apps/ket-android && ./gradlew --no-daemon assembleDebug)
./packaging/validate-android-apk.sh --abi arm64-v8a apps/ket-android/app/build/outputs/apk/debug/app-arm64-v8a-debug.apk
./packaging/validate-android-apk.sh apps/ket-android/app/build/outputs/apk/debug/app-universal-debug.apk
```

The root runner and maintained feature-to-test matrix are described in
[the testing guide](TESTING.md). It intentionally keeps package installation,
signing, real-device, and restricted-network evidence as separate release gates.

Persistent Android and OpenPGP identities, package-signing commands, and user verification are documented in [release signing](RELEASE_SIGNING.md).

Run the Windows NSIS lifecycle only from an elevated shell on a disposable test host:

```powershell
$env:KET_PACKAGE_TEST_ALLOW_HOST_MUTATION = "1"
$installer = Get-ChildItem target/release/bundle/nsis/*.exe -File
./packaging/verify-windows-nsis.ps1 -Installer $installer.FullName
```

For the complete physical-host checklist, evidence capture, protocol matrix, reconnect checks, and manual uninstall procedure, use [the Windows package test plan](WINDOWS_TESTING.md).

For a production server, source `.env`, run `./packaging/validate-env.sh`, and validate the base file plus each enabled overlay with `docker compose config --quiet`. The preflight validates client-visible URL, node identity/location, and enabled transport inputs; `ket-server` then repeats authoritative structured URL and manifest-field validation before it binds a listener. The local OpenVPN handshake check uses `dev null`; it does not replace a TUN-backed packet-flow test. The local Shadowsocks harness proves TCP through real upstream engines without the production egress ACL. The Oracle ARM64 deployment separately passed authenticated public TCP egress and UDP DNS forwarding through the production Shadowsocks listener and ACL. The XHTTP harness proves certificate-verified TLS, real XHTTP traffic, counters, and revocation through a local stunnel intermediary. A production-profile SOCKS matrix and an isolated Linux TUN test additionally carried Google, Wikipedia, GitHub, Cloudflare, Debian, Mozilla, and Microsoft traffic after the broker took explicit ownership of host DNS; this fixed the previous Linux behavior where only locally resolvable sites worked. The production Cloudflare route also passed the owner-signed Android restricted-network HTTPS gate, while sustained provider-limit testing remains pending. Hysteria2 requires direct UDP reachability; VLESS + REALITY and OpenVPN/stunnel require separate direct raw TCP listeners. XHTTP/TLS Stealth and WireGuard TLS instead require path-specific Cloudflare or compatible HTTP routes to separate loopback-only origins. WireGuard TLS and OpenVPN require a rootful Linux Docker host with `/dev/net/tun`; WireGuard additionally needs kernel WireGuard support.

Container upgrades retain the v1 state volume. The loader accepts older v1 session records that predate scoped data-plane hashes, but those missing credentials remain fail closed until the client creates a current session. Unknown schema versions and structurally inconsistent or oversized state files stop startup instead of discarding grants or guessing a migration. Back up the `ket-state` volume before upgrading; never replace a rejected state file with an empty document as an automated recovery action.

## Release candidate

A release is tagged only after the exact commit has run on a real server. Stubs in the test suite cannot see real DNS, real ports, or a CDN or tunnel in front of the server; v1.1.0 needed three fixes right after its tag because it met a real server only then.

1. Commit the release: version bump, `releases.json` entry, and docs. Push it and wait for Ket CI to pass.
2. Run `scripts/release-preflight.sh local`. It checks that every version file agrees and the tag is free, that CI passed for this commit, the full test suite, the real installer (automatic address, an unresolvable domain), and the real bootstrap downloading this commit from GitHub.
3. Deploy the candidate to the canary server by commit, with no tag yet: `sudo ket update --to <commit>`. A commit has no published image, so the canary builds it locally, which also exercises that fallback.
4. Run `scripts/release-preflight.sh all <ssh-target> [--ssh-key FILE]`. On top of the local checks, it confirms the canary runs this commit, `ket doctor` passes, every operational endpoint answers 404 through the public name (so a CDN or tunnel cannot bypass the edge), and a fresh invite enrolls with the shipped client code, reports this version, and is revoked.
5. When the release touches the desktop tunnel, also run `scripts/dns-leak-lab.sh <ssh-target> [--ssh-key FILE]` and `scripts/roaming-lab.sh <ssh-target> [--ssh-key FILE]` with every tunnel protocol the canary offers ([DNS_LEAKS.md](DNS_LEAKS.md), [CLIENT_CORE.md](CLIENT_CORE.md#changing-networks)); they must end with `No DNS leak in any setup` and `Every protocol survived the network changes without a leak`.
6. Only when the preflight prints `READY`, tag that same commit and build the signed assets from it. Any failure means a fix, a new candidate commit, and the steps again; never a fix after the tag.
7. After publishing, run `sudo ket update` on the canary (it moves from the commit to the tag and pulls the published image) and rerun step 4.

## Update ratings

Before tagging, add the release to `crates/ket-update/releases.json` with its server rating and a rating for each app ([choosing a rating](UPDATES.md#choosing-a-rating)). Servers refuse apps based on these ratings, and servers with automatic updates apply `security` and `critical` releases on their own, so a wrong rating reaches users within a day.

## Server installer

The one-command bootstrap (`install.sh`) installs whatever GitHub marks as the latest release, using that release's own `packaging/install-server.sh`. Publishing a release therefore changes what every new server gets, immediately. Before publishing, run `./scripts/test-all.sh server` and `./scripts/test-all.sh integration` on the release commit, confirm `packaging/installer/protocol` is present, and preview the installer on the tag with `KET_REF=<tag>` and `--plan`. Releases without the protocol file are installed through their own legacy command, which the bootstrap prints.

## Signing

The Android debug artifact is only for testing. Production Android, Linux, and Windows artifacts must be signed by the release owner and their checksums published alongside the files. Ket does not store signing keys in this repository.

Android release tasks require `KET_ANDROID_KEYSTORE`, `KET_ANDROID_KEYSTORE_PASSWORD`, `KET_ANDROID_KEY_ALIAS`, and `KET_ANDROID_KEY_PASSWORD`. `packaging/build-android.sh release` additionally requires `KET_ANDROID_CERT_SHA256` and refuses an APK whose signer differs from that pinned certificate. The Android version comes from the workspace version (`versionCode` = MAJOR×10000 + MINOR×100 + PATCH); `build-android.sh release` refuses a `KET_ANDROID_VERSION_NAME` or `KET_ANDROID_VERSION_CODE` that disagrees, so an old env file cannot mislabel a release. The CI Android job builds the release variant with an ephemeral key and validates both shipped splits and their fingerprint, then deliberately uploads only the debug APKs.

The maintained server and Linux/Windows data planes are Hysteria2, Xray-core VLESS + REALITY, Xray-core VLESS + XHTTP/TLS Stealth, shadowsocks-rust Shadowsocks 2022, WireGuard over wstunnel WebSocket/TLS, and OpenVPN over stunnel TLS. Hysteria XOR is a selectable Salamander/Gecko mode over the same authenticated encrypted Hysteria data plane, not a separate security protocol. SOCKS-backed desktop transports share a locally tested full-route bridge; OpenVPN owns a separately supervised native TUN. Android implements the same choices through platform-owned routing, including a private OpenVPN management/TUN bridge and certificate-pinned stunnel-compatible TLS carrier. Android startup fallback, engine-exit recovery, bidirectional Wi-Fi/cellular recovery, cancellation, and disconnect have been exercised on a physical current arm64 device. On 2026-07-22, an owner-signed APK passed restricted-network VLESS + REALITY, XHTTP/TLS Stealth, and Shadowsocks routes with bridge-local mapped DNS and the end-to-end HTTPS gate. The repeated REALITY run exited through `161.118.195.126`, Android independently validated the full-route VPN, the UI recorded megabytes of bidirectional transfer, and disconnect restored direct device egress. XHTTP also has local certificate-verified TLS traffic, scoped UUID, counters, release, and revocation proof through real Xray engines. Shadowsocks key enforcement, counters, and revocation are proven locally; the Oracle deployment additionally passed authenticated public TCP egress and UDP DNS forwarding, followed by listener removal on grant revocation. WireGuard enrollment and peer revocation, plus the OpenVPN agent/stunnel stack, are deployed on Oracle ARM64. OpenVPN has local lifecycle proof and a public certificate-pinned nested-TLS handshake with scoped authentication and nonzero telemetry after strict trust-chain repair. Restricted-network Android gates remain for Hysteria2, WireGuard, and OpenVPN, along with TUN-backed public OpenVPN packet flow, fail-closed handover, Doze-exit renewal, graceful permission-revoke, always-on/reboot, API 26, connected-state DNS-leak, lockdown, sustained transfer, and owner-signed Windows installation. IKEv2 remains a contract-level extension point rather than an implemented adapter.

## Server image

Pushing a `v*` tag runs [`server-image.yml`](../.github/workflows/server-image.yml): it checks that the tag matches the workspace version and the last `releases.json` entry, builds `linux/amd64` and `linux/arm64` on native runners, publishes `ghcr.io/emp5r0r/ket-control-plane:<version>` with an `org.opencontainers.image.version` label and a build provenance attestation, and touches nothing else. Installs and updates of that release pull it and fall back to building on the server when it is missing or its label does not match, so a failed image job only makes installs slower.

The first time the package is published, check its visibility under the repository's **Packages** and make it public, or servers cannot pull it. Verify an image with `gh attestation verify oci://ghcr.io/emp5r0r/ket-control-plane:<version> --repo Emp5r0R/Ket`.

To publish an image by hand instead (for a fork, for example):

```bash
./packaging/build-multiarch.sh ghcr.io/your-org/ket-control-plane:<tag>
```

The script publishes `linux/amd64` and `linux/arm64`. Docker selects the matching image on the host. The command requires Buildx and registry push permission; it does not alter the local Docker daemon image store.
