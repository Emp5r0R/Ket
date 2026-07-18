#!/usr/bin/env bash
set -euo pipefail

version=v26.3.27
target=${1:-}
destination=${2:-}
android_target=false

if [[ -z ${target} || -z ${destination} ]]; then
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64|android-amd64|android-arm64> <destination>\n' "$0" >&2
  exit 2
fi

case "${target}" in
  linux-amd64)
    asset=Xray-linux-64.zip
    executable=xray
    checksum=23cd9af937744d97776ee35ecad4972cf4b2109d1e0fe6be9930467608f7c8ae
    ;;
  linux-arm64)
    asset=Xray-linux-arm64-v8a.zip
    executable=xray
    checksum=4d30283ae614e3057f730f67cd088a42be6fdf91f8639d82cb69e48cde80413c
    ;;
  windows-amd64)
    asset=Xray-windows-64.zip
    executable=xray.exe
    checksum=d004c39288ce9ada487c6f398c7c545f7d749e44bdfdd59dbc9f865afba4e1ad
    ;;
  android-amd64)
    asset=Xray-android-amd64.zip
    executable=xray
    checksum=bfa1dd4cb3cd94f92af6c718ac6933cc84bd3ddd8343aa1e413162152930bee5
    android_target=true
    ;;
  android-arm64)
    asset=Xray-android-arm64-v8a.zip
    executable=xray
    checksum=57149ffd48b629c07bf76938e73ab2729fde5910091497eab3e93d1c190f4c1b
    android_target=true
    ;;
  *)
    printf 'Unsupported Xray target: %s\n' "${target}" >&2
    exit 2
    ;;
esac

archive=$(mktemp)
binary=$(mktemp)
cleanup() {
  rm -f "$archive" "$binary"
}
trap cleanup EXIT
url="https://github.com/XTLS/Xray-core/releases/download/${version}/${asset}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --output "$archive" "$url"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
unzip -p "$archive" "$executable" > "$binary"
if [[ "$android_target" == true ]]; then
  mkdir -p "$(dirname "$destination")"
  cp "$binary" "$destination"
else
  mkdir -p "$(dirname "$destination")"
  cp "$binary" "$destination"
  chmod 0755 "$destination" 2>/dev/null || [[ -x "$destination" ]]
fi
printf 'Fetched and verified Xray %s for %s.\n' "$version" "$target"
