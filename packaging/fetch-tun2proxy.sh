#!/usr/bin/env bash
set -euo pipefail

version=v0.8.2
target=${1:-}
destination=${2:-}
wintun_destination=${3:-}

if [[ -z ${target} || -z ${destination} ]]; then
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64> <destination> [wintun-destination]\n' "$0" >&2
  exit 2
fi

case "${target}" in
  linux-amd64)
    asset=tun2proxy-x86_64-unknown-linux-gnu.zip
    executable=tun2proxy-bin
    checksum=ed4ddd62a3d58f1894262b4ef55fa800038664e67a86cbff047d3214e236d5bd
    ;;
  linux-arm64)
    asset=tun2proxy-aarch64-unknown-linux-gnu.zip
    executable=tun2proxy-bin
    checksum=540194769887bb75dc68f75b42807ccbfdd4a5fbdbef33d73bfbd3da58290317
    ;;
  windows-amd64)
    asset=tun2proxy-x86_64-pc-windows-msvc.zip
    executable=tun2proxy-bin.exe
    checksum=8f002a9a100d6739814aa376e8380de082964cd1c58dc9fd407171bebc34c218
    if [[ -z "$wintun_destination" ]]; then
      printf 'The Windows target requires a wintun destination.\n' >&2
      exit 2
    fi
    ;;
  *)
    printf 'Unsupported tun2proxy target: %s\n' "${target}" >&2
    exit 2
    ;;
esac

archive=$(mktemp)
binary=$(mktemp)
wintun=$(mktemp)
cleanup() {
  rm -f "$archive" "$binary" "$wintun"
}
trap cleanup EXIT
url="https://github.com/tun2proxy/tun2proxy/releases/download/${version}/${asset}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
  --speed-limit 1024 --speed-time 30 \
  --output "$archive" "$url"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
unzip -p "$archive" "$executable" > "$binary"
mkdir -p "$(dirname "$destination")"
cp "$binary" "$destination"
chmod 0755 "$destination" 2>/dev/null || [[ -x "$destination" ]]
if [[ "$target" == windows-amd64 ]]; then
  unzip -p "$archive" wintun.dll > "$wintun"
  mkdir -p "$(dirname "$wintun_destination")"
  cp "$wintun" "$wintun_destination"
fi
printf 'Fetched and verified tun2proxy %s for %s.\n' "$version" "$target"
