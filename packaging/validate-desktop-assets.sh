#!/usr/bin/env bash
set -euo pipefail

platform=${1:-}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
notice="$repo_root/THIRD_PARTY_NOTICES.md"

[[ -s "$notice" ]] || {
  printf 'Missing desktop third-party notice: %s\n' "$notice" >&2
  exit 1
}

case "$platform" in
  linux)
    engine="$repo_root/apps/ket-desktop/src-tauri/binaries/hysteria"
    shadowsocks="$repo_root/apps/ket-desktop/src-tauri/binaries/sslocal"
    xray="$repo_root/apps/ket-desktop/src-tauri/binaries/xray"
    wstunnel="$repo_root/apps/ket-desktop/src-tauri/binaries/wstunnel"
    bridge="$repo_root/apps/ket-desktop/src-tauri/binaries/tun2proxy"
    service="$repo_root/target/release/ket-tunnel-service"
    ;;
  windows)
    engine="$repo_root/apps/ket-desktop/src-tauri/binaries/hysteria.exe"
    shadowsocks="$repo_root/apps/ket-desktop/src-tauri/binaries/sslocal.exe"
    xray="$repo_root/apps/ket-desktop/src-tauri/binaries/xray.exe"
    wstunnel="$repo_root/apps/ket-desktop/src-tauri/binaries/wstunnel.exe"
    bridge="$repo_root/apps/ket-desktop/src-tauri/binaries/tun2proxy.exe"
    wintun="$repo_root/apps/ket-desktop/src-tauri/binaries/wintun.dll"
    service="$repo_root/target/release/ket-tunnel-service.exe"
    installer="$repo_root/packaging/windows/install-tunnel-service.ps1"
    hooks="$repo_root/apps/ket-desktop/src-tauri/windows/hooks.nsh"
    ;;
  *)
    printf 'Usage: %s <linux|windows>\n' "$0" >&2
    exit 2
    ;;
esac

for asset in "$engine" "$shadowsocks" "$xray" "$wstunnel" "$bridge" "$service"; do
  if [[ ! -f "$asset" ]]; then
    printf 'Missing desktop bundle asset: %s\n' "$asset" >&2
    exit 1
  fi
  if [[ ! -s "$asset" ]]; then
    printf 'Desktop bundle asset is empty: %s\n' "$asset" >&2
    exit 1
  fi
done

if [[ "$platform" == windows ]]; then
  for asset in "$wintun" "$installer" "$hooks"; do
    if [[ ! -s "$asset" ]]; then
      printf 'Missing Windows installer asset: %s\n' "$asset" >&2
      exit 1
    fi
  done
fi

if [[ "$platform" == linux ]]; then
  for asset in "$service" "$engine" "$shadowsocks" "$xray" "$wstunnel" "$bridge"; do
    if [[ ! -x "$asset" ]]; then
      printf 'Linux tunnel asset is not executable: %s\n' "$asset" >&2
      exit 1
    fi
  done
fi

printf 'Desktop assets ready for %s packaging.\n' "$platform"
