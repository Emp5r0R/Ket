#!/usr/bin/env bash
set -euo pipefail

version=1.24.0
target=${1:-}
destination=${2:-}
component=${3:-sslocal}
android_target=false

if [[ -z ${target} || -z ${destination} ]]; then
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64|android-amd64|android-arm64> <destination> [sslocal|ssmanager]\n' "$0" >&2
  exit 2
fi

case "$target" in
  linux-amd64)
    triple=x86_64-unknown-linux-gnu
    checksum=5f528efb4e51e732352f5c69538dcc76e8cf8f6d1a240dfb5b748a67f0b05f65
    format=tar.xz
    ;;
  linux-arm64)
    triple=aarch64-unknown-linux-gnu
    checksum=dc56150cb263e1e150af33cc4c6542035aab3edf602e340842cca4138a4d5c51
    format=tar.xz
    ;;
  windows-amd64)
    triple=x86_64-pc-windows-msvc
    checksum=8f4bdd02cf3b42976f6b48e01239bc0ae61f9da7a3c260505a7880de615291d0
    format=zip
    executable=sslocal.exe
    ;;
  android-amd64)
    triple=x86_64-linux-android
    checksum=8e4f2d905a2db5a63e83d764eb80d82ed78867a2e8ea2bfbd88dfe478b1f1f1d
    format=tar.xz
    executable=sslocal
    android_target=true
    ;;
  android-arm64)
    triple=aarch64-linux-android
    checksum=7cbbea91a4a411506ae0afa9d620d98c00e63b5e291203ce9ec3c271e6d7453d
    format=tar.xz
    executable=sslocal
    android_target=true
    ;;
  *)
    printf 'Unsupported Shadowsocks target: %s\n' "$target" >&2
    exit 2
    ;;
esac

case "$target:$component" in
  linux-amd64:sslocal|linux-arm64:sslocal) executable=sslocal ;;
  linux-amd64:ssmanager|linux-arm64:ssmanager) executable=ssmanager ;;
  windows-amd64:sslocal) executable=sslocal.exe ;;
  android-amd64:sslocal|android-arm64:sslocal) executable=sslocal ;;
  *)
    printf 'Unsupported Shadowsocks component %s for %s.\n' "$component" "$target" >&2
    exit 2
    ;;
esac

asset="shadowsocks-v${version}.${triple}.${format}"
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
archive="$temporary/$asset"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
  --speed-limit 1024 --speed-time 30 \
  --output "$archive" \
  "https://github.com/shadowsocks/shadowsocks-rust/releases/download/v${version}/${asset}"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
case "$format" in
  tar.xz) tar --extract --xz --file "$archive" --directory "$temporary" "$executable" ;;
  zip) unzip -q "$archive" "$executable" -d "$temporary" ;;
esac

if [[ "$android_target" == true ]]; then
  mkdir -p "$(dirname "$destination")"
  cp "$temporary/$executable" "$destination"
else
  install -D -m 0755 "$temporary/$executable" "$destination"
fi
printf 'Fetched and verified shadowsocks-rust %s %s for %s.\n' "$version" "$component" "$target"
