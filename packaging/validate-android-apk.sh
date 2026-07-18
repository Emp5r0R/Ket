#!/usr/bin/env bash
set -euo pipefail

apk=${1:-}
if [[ -z "$apk" || ! -f "$apk" ]]; then
  printf 'Usage: %s <apk>\n' "$0" >&2
  exit 2
fi

for abi in armeabi-v7a arm64-v8a x86 x86_64; do
  for library in libhysteria.so libhev-socks5-tunnel.so libket-android-native.so; do
    entry="lib/$abi/$library"
    if ! unzip -Z1 "$apk" "$entry" | grep -Fxq "$entry"; then
      printf 'APK is missing %s\n' "$entry" >&2
      exit 1
    fi
  done
done

apksigner=${APKSIGNER:-}
if [[ -z "$apksigner" ]] && command -v apksigner >/dev/null 2>&1; then
  apksigner=$(command -v apksigner)
fi
if [[ -n "$apksigner" ]]; then
  "$apksigner" verify "$apk"
fi

printf 'Validated Android transport payloads for armeabi-v7a, arm64-v8a, x86, and x86_64.\n'
