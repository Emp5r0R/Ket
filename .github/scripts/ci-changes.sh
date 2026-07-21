#!/usr/bin/env bash
set -euo pipefail

jobs=(
  rust
  desktop_ui
  desktop_native
  linux_package
  desktop_windows
  windows_package
  android
  container
)

declare -A selected=()
for job in "${jobs[@]}"; do
  selected["$job"]=false
done

enable() {
  local job
  for job in "$@"; do
    selected["$job"]=true
  done
}

enable_all() {
  enable "${jobs[@]}"
}

classify() {
  local path=${1#./}

  case "$path" in
    .github/*)
      enable_all
      ;;
    Cargo.toml | Cargo.lock | rust-toolchain* | .cargo/*)
      enable rust desktop_native linux_package desktop_windows windows_package container
      ;;
    crates/ket-server/*)
      enable rust container
      ;;
    crates/ket-core/* | crates/ket-tunnel-protocol/*)
      enable rust desktop_native linux_package desktop_windows windows_package container
      ;;
    crates/ket-client-core/* | crates/ket-tunnel-service/*)
      enable rust desktop_native linux_package desktop_windows windows_package
      ;;
    crates/*)
      enable rust desktop_native linux_package desktop_windows windows_package container
      ;;
    apps/ket-desktop/src/* | apps/ket-desktop/public/* | apps/ket-desktop/index.html)
      enable desktop_ui linux_package windows_package
      ;;
    apps/ket-desktop/src-tauri/*)
      enable desktop_native linux_package desktop_windows windows_package
      ;;
    apps/ket-desktop/package.json | apps/ket-desktop/package-lock.json | apps/ket-desktop/tsconfig*.json | apps/ket-desktop/vite.config.* | apps/ket-desktop/vitest.config.*)
      enable desktop_ui desktop_native linux_package desktop_windows windows_package
      ;;
    apps/ket-desktop/*)
      enable desktop_ui desktop_native linux_package desktop_windows windows_package
      ;;
    apps/ket-android/*)
      enable android
      ;;
    packaging/build-android.sh | packaging/prepare-android-engines.sh | packaging/validate-android-apk.sh)
      enable android
      ;;
    packaging/fetch-hysteria.sh | packaging/fetch-shadowsocks.sh | packaging/fetch-xray.sh)
      enable linux_package windows_package android
      ;;
    packaging/fetch-tun2proxy.sh | packaging/validate-desktop-assets.sh)
      enable linux_package windows_package
      ;;
    packaging/verify-linux-deb.sh)
      enable linux_package
      ;;
    packaging/verify-windows-nsis.ps1)
      enable windows_package
      ;;
    packaging/linux/*)
      enable linux_package
      ;;
    packaging/windows/*)
      enable windows_package
      ;;
    packaging/build-multiarch.sh | packaging/validate-env.sh | Dockerfile | compose*.yaml | .env.example)
      enable container
      ;;
    README.md | docs/* | assets/* | .gitignore | LICENSE*)
      ;;
    *)
      # Unknown files take the conservative path so new components cannot bypass CI.
      enable_all
      ;;
  esac
}

case "${1:-}" in
  --all)
    enable_all
    ;;
  --null)
    while IFS= read -r -d '' path; do
      classify "$path"
    done
    ;;
  "")
    while IFS= read -r path; do
      [[ -n "$path" ]] && classify "$path"
    done
    ;;
  *)
    printf 'Usage: %s [--all|--null]\n' "$0" >&2
    exit 2
    ;;
esac

for job in "${jobs[@]}"; do
  printf '%s=%s\n' "$job" "${selected[$job]}"
done
