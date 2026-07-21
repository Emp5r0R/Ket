#!/usr/bin/env bash
set -euo pipefail

expected_cert_sha256=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --expected-cert-sha256)
      if [[ $# -lt 2 ]]; then
        printf '%s requires a value.\n' "$1" >&2
        exit 2
      fi
      expected_cert_sha256=$2
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

apk=${1:-}
if [[ -z "$apk" || ! -f "$apk" ]]; then
  printf 'Usage: %s [--expected-cert-sha256 <digest>] <apk>\n' "$0" >&2
  exit 2
fi
if [[ $# -ne 1 ]]; then
  printf 'Usage: %s [--expected-cert-sha256 <digest>] <apk>\n' "$0" >&2
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

entry="lib/arm64-v8a/libwstunnel.so"
if ! unzip -Z1 "$apk" "$entry" | grep -Fxq "$entry"; then
  printf 'APK is missing %s\n' "$entry" >&2
  exit 1
fi

for abi in arm64-v8a x86_64; do
  for library in libsslocal.so libxray.so; do
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
if [[ -z "$apksigner" && -n "$expected_cert_sha256" ]]; then
  printf 'apksigner is required when an expected signer certificate is supplied.\n' >&2
  exit 1
fi
if [[ -n "$apksigner" ]]; then
  signer_output=$("$apksigner" verify --verbose --print-certs "$apk")
  printf '%s\n' "$signer_output"
  if [[ -n "$expected_cert_sha256" ]]; then
    signer_count=$(printf '%s\n' "$signer_output" | sed -n 's/^Number of signers: //p' | head -n 1)
    if [[ "$signer_count" != "1" ]]; then
      printf 'Pinned release APK must have exactly one signer; found %s.\n' \
        "${signer_count:-an unknown count}" >&2
      exit 1
    fi
    expected_cert_sha256=${expected_cert_sha256//:/}
    expected_cert_sha256=${expected_cert_sha256//[[:space:]]/}
    expected_cert_sha256=${expected_cert_sha256,,}
    if [[ ! "$expected_cert_sha256" =~ ^[0-9a-f]{64}$ ]]; then
      printf 'Expected signer SHA-256 must contain exactly 64 hexadecimal digits.\n' >&2
      exit 2
    fi
    actual_cert_sha256=$(printf '%s\n' "$signer_output" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' | head -n 1)
    actual_cert_sha256=${actual_cert_sha256//:/}
    actual_cert_sha256=${actual_cert_sha256//[[:space:]]/}
    actual_cert_sha256=${actual_cert_sha256,,}
    if [[ "$actual_cert_sha256" != "$expected_cert_sha256" ]]; then
      printf 'APK signer certificate mismatch: expected %s, found %s.\n' \
        "$expected_cert_sha256" "${actual_cert_sha256:-none}" >&2
      exit 1
    fi
  fi
fi

printf 'Validated Android Hysteria payloads for four ABIs, Shadowsocks/Xray payloads for arm64-v8a and x86_64, and wstunnel for arm64-v8a.\n'
