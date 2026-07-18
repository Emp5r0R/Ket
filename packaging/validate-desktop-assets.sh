#!/usr/bin/env bash
set -euo pipefail

platform=${1:-}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

case "$platform" in
  linux)
    engine="$repo_root/apps/ket-desktop/src-tauri/binaries/hysteria"
    service="$repo_root/target/release/ket-tunnel-service"
    ;;
  windows)
    engine="$repo_root/apps/ket-desktop/src-tauri/binaries/hysteria.exe"
    service="$repo_root/target/release/ket-tunnel-service.exe"
    installer="$repo_root/packaging/windows/install-tunnel-service.ps1"
    hooks="$repo_root/apps/ket-desktop/src-tauri/windows/hooks.nsh"
    ;;
  *)
    printf 'Usage: %s <linux|windows>\n' "$0" >&2
    exit 2
    ;;
esac

for asset in "$engine" "$service"; do
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
  for asset in "$installer" "$hooks"; do
    if [[ ! -s "$asset" ]]; then
      printf 'Missing Windows installer asset: %s\n' "$asset" >&2
      exit 1
    fi
  done
fi

if [[ "$platform" == linux && ! -x "$service" ]]; then
  printf 'Linux tunnel service is not executable: %s\n' "$service" >&2
  exit 1
fi

printf 'Desktop assets ready for %s packaging.\n' "$platform"
