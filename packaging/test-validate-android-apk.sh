#!/usr/bin/env bash
# Tests validate-android-apk.sh against fake universal and per-ABI APKs.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
validator="$root/packaging/validate-android-apk.sh"
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
# A stand-in apksigner that reports one signer with the digest in $scratch/signer.
digest=$(printf 'ab%.0s' {1..32})
printf '%s\n' "$digest" > "$scratch/signer"
cat > "$scratch/apksigner" <<STUB
#!/usr/bin/env bash
printf 'Verifies\nNumber of signers: 1\nSigner #1 certificate SHA-256 digest: %s\n' "\$(cat "$scratch/signer")"
STUB
chmod +x "$scratch/apksigner"
export APKSIGNER="$scratch/apksigner"

common=(libhysteria.so libhev-socks5-tunnel.so libket-android-native.so libopenvpn.so libovpnexec.so)

# libraries ABI -> the library list the validator requires for that ABI.
libraries() {
  printf '%s\n' "${common[@]}"
  case $1 in
    arm64-v8a) printf '%s\n' libsslocal.so libxray.so libwstunnel.so ;;
    x86_64) printf '%s\n' libsslocal.so libxray.so ;;
  esac
}

# make_apk NAME ABI... -> builds a zip with every required library for each ABI.
make_apk() {
  local apk="$scratch/$1.apk" abi library
  shift
  {
    printf 'AndroidManifest.xml\n'
    for abi in "$@"; do
      while IFS= read -r library; do printf 'lib/%s/%s\n' "$abi" "$library"; done < <(libraries "$abi")
    done
  } > "$apk.list"
  python3 -I -c '
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as apk:
    for name in open(sys.argv[2]).read().split():
        apk.writestr(name, b"x")
' "$apk" "$apk.list"
  printf '%s\n' "$apk"
}

# drop_entry APK ENTRY -> rewrites APK without ENTRY.
drop_entry() {
  python3 -I -c '
import sys, zipfile
names = [n for n in zipfile.ZipFile(sys.argv[1]).namelist() if n != sys.argv[2]]
with zipfile.ZipFile(sys.argv[1], "w") as apk:
    for name in names:
        apk.writestr(name, b"x")
' "$1" "$2"
}

pass() { "$validator" "$@" >/dev/null || { printf 'expected success: %s\n' "$*" >&2; exit 1; }; }
reject() {
  local expected=$1
  shift
  local output
  if output=$("$validator" "$@" 2>&1); then
    printf 'expected failure: %s\n' "$*" >&2
    exit 1
  fi
  grep -Fq -- "$expected" <<<"$output" || { printf 'expected "%s" in: %s\n' "$expected" "$output" >&2; exit 1; }
}

universal=$(make_apk universal armeabi-v7a arm64-v8a x86 x86_64)
arm64=$(make_apk arm64 arm64-v8a)
armv7=$(make_apk armv7 armeabi-v7a)

pass "$universal"
pass --abi universal "$universal"
pass --abi arm64-v8a "$arm64"
pass --abi armeabi-v7a "$armv7"

# A split is not a universal APK, and a universal APK is not a split.
reject 'APK is missing lib/armeabi-v7a/' "$arm64"
reject 'also carries libraries for: armeabi-v7a x86 x86_64' --abi arm64-v8a "$universal"
reject 'Unknown ABI: mips' --abi mips "$arm64"
reject 'Usage:' --abi arm64-v8a

# Each ABI's engines are required.
drop_entry "$arm64" lib/arm64-v8a/libwstunnel.so
reject 'APK is missing lib/arm64-v8a/libwstunnel.so' --abi arm64-v8a "$arm64"
drop_entry "$universal" lib/x86_64/libxray.so
reject 'APK is missing lib/x86_64/libxray.so' "$universal"

# The signer must match the pinned certificate.
pass --abi armeabi-v7a --expected-cert-sha256 "${digest^^}" "$armv7"
printf '%s\n' "$(printf 'cd%.0s' {1..32})" > "$scratch/signer"
reject 'APK signer certificate mismatch' --abi armeabi-v7a --expected-cert-sha256 "$digest" "$armv7"

printf 'Android APK validator tests passed.\n'
