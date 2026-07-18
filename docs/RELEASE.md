# Release Checklist

## Artifacts

| Artifact | Build path | Current status |
| --- | --- | --- |
| Rust control plane image | `docker build --pull -t ket-control-plane:<tag> .` | Verified on Oracle ARM64 host |
| Linux desktop `.deb` | CI job `linux-package` | Built by Tauri with the pinned Hysteria engine |
| Windows desktop NSIS installer | CI job `windows-package` | Built by Tauri with the pinned Hysteria engine |
| Android debug APK | `./packaging/build-android.sh` | Locally built and APK signature verified |

## Required checks

Run these before publishing a release:

```bash
cargo fmt --all -- --check
cargo test --locked --workspace --exclude ket-desktop
cargo clippy --locked --workspace --exclude ket-desktop --all-targets --all-features -- -D warnings
npm --prefix apps/ket-desktop test -- --run
npm --prefix apps/ket-desktop run build
```

For a production server, source `.env`, run `./packaging/validate-env.sh`, and validate both Compose files with `docker compose config --quiet`. Hysteria2 requires a DNS-only UDP hostname or a compatible Layer 4 proxy; ordinary Cloudflare HTTP proxying does not carry its UDP data plane.

## Signing

The Android debug artifact is only for testing. Production Android, Linux, and Windows artifacts must be signed by the release owner and their checksums published alongside the files. Ket does not store signing keys in this repository.

The current maintained data-plane integration is Hysteria2. Other protocol identifiers are contract-level extension points and must not be advertised as active until their maintained upstream engines and platform adapters are integrated and tested.
