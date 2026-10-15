#!/usr/bin/env bash
# Checks a Ket APK's native engine payload and, optionally, its signer.
#
#   validate-android-apk.sh [--abi universal|ABI] [--expected-cert-sha256 DIGEST] APK
#
# --abi names the APK split: "universal" (the default) must carry every ABI;
# a single ABI must carry exactly that ABI's libraries and nothing else.
set -euo pipefail

readonly ABIS=(armeabi-v7a arm64-v8a x86 x86_64)

usage() {
  printf 'Usage: %s [--abi universal|%s] [--expected-cert-sha256 <digest>] <apk>\n' \
    "$0" "$(IFS='|'; printf '%s' "${ABIS[*]}")" >&2
  exit 2
}

# Libraries every build of Ket must bundle for one ABI. The Go and Rust engines
# that only ship for 64-bit ABIs are listed per ABI.
required_libraries() {
  printf '%s\n' libhysteria.so libhev-socks5-tunnel.so libket-android-native.so libopenvpn.so libovpnexec.so
  case $1 in
    arm64-v8a) printf '%s\n' libsslocal.so libxray.so libwstunnel.so ;;
    x86_64) printf '%s\n' libsslocal.so libxray.so ;;
  esac
}

expected_cert_sha256=""
split=universal
while [[ $# -gt 0 ]]; do
  case "$1" in
    --expected-cert-sha256 | --abi)
      [[ $# -ge 2 ]] || { printf '%s requires a value.\n' "$1" >&2; exit 2; }
      if [[ $1 == --abi ]]; then split=$2; else expected_cert_sha256=$2; fi
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
[[ -n "$apk" && -f "$apk" && $# -eq 1 ]] || usage

if [[ $split == universal ]]; then
  expected_abis=("${ABIS[@]}")
elif printf '%s\n' "${ABIS[@]}" | grep -Fxq -- "$split"; then
  expected_abis=("$split")
else
  printf 'Unknown ABI: %s\n' "$split" >&2
  usage
fi

entries=$(unzip -Z1 "$apk") || { printf 'Not a readable APK: %s\n' "$apk" >&2; exit 1; }
for abi in "${expected_abis[@]}"; do
  while IFS= read -r library; do
    entry="lib/$abi/$library"
    if ! grep -Fxq -- "$entry" <<<"$entries"; then
      printf 'APK is missing %s\n' "$entry" >&2
      exit 1
    fi
  done < <(required_libraries "$abi")
done

present_abis=$(sed -n 's|^lib/\([^/]*\)/.*|\1|p' <<<"$entries" | LC_ALL=C sort -u)
unexpected=$(LC_ALL=C comm -23 <(printf '%s\n' "$present_abis") \
  <(printf '%s\n' "${expected_abis[@]}" | LC_ALL=C sort -u))
if [[ -n $unexpected ]]; then
  printf 'The %s APK also carries libraries for: %s\n' "$split" "$(tr '\n' ' ' <<<"$unexpected")" >&2
  exit 1
fi

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

printf 'Validated the %s APK engine payload for: %s\n' "$split" "${expected_abis[*]}"
