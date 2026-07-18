#!/usr/bin/env bash
set -euo pipefail

version=v2.10.0
target=${1:-}
destination=${2:-}
android_target=false

if [[ -z ${target} || -z ${destination} ]]; then
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64|windows-arm64|android-386|android-amd64|android-arm64|android-armv7> <destination>\n' "$0" >&2
  exit 2
fi

case "${target}" in
  linux-amd64)
    asset=hysteria-linux-amd64
    checksum=04f7804159ef1d798de12a817d73aab4b9040ebe45fc62e223000c5c59e987fe
    ;;
  linux-arm64)
    asset=hysteria-linux-arm64
    checksum=8995b33085f7b07769955e23c1c53468064ebf6c408b1d7b663044556898426a
    ;;
  windows-amd64)
    asset=hysteria-windows-amd64.exe
    checksum=a0b4b1851919235b9424632b894b5232eec861c1c20e955e82e3dbc6698490d0
    ;;
  windows-arm64)
    asset=hysteria-windows-arm64.exe
    checksum=ea1d6123620aa8c79d6e5409372524a0f7f7d9c7cc60c5c40fdcff1a12466b8d
    ;;
  android-386)
    asset=hysteria-android-386
    checksum=4e954aed6530e4eff0cee5cafabb443b479dc6346c9e9edfd2e1388563a601c4
    android_target=true
    ;;
  android-amd64)
    asset=hysteria-android-amd64
    checksum=1eae1aa7b9b6e332037887ab3dce79d66b19e9b49f693876e7fdb50540bae1c2
    android_target=true
    ;;
  android-arm64)
    asset=hysteria-android-arm64
    checksum=2fc53c30df5bcf09dc69e7840ca251db596bd38883be878e96de7a860220eeb4
    android_target=true
    ;;
  android-armv7)
    asset=hysteria-android-armv7
    checksum=e2f0681ef58bdd3575030fa41cf2b637491ac19822964a0dd2d89370da56fd5c
    android_target=true
    ;;
  *)
    printf 'Unsupported Hysteria target: %s\n' "${target}" >&2
    exit 2
    ;;
esac

temporary=$(mktemp)
trap 'rm -f "${temporary}"' EXIT
url="https://github.com/apernet/hysteria/releases/download/app/${version}/${asset}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --output "${temporary}" "${url}"
printf '%s  %s\n' "${checksum}" "${temporary}" | sha256sum --check --status
if [[ "$android_target" == true ]]; then
  mkdir -p "$(dirname "$destination")"
  cp "$temporary" "$destination"
else
  install -D -m 0755 "${temporary}" "${destination}"
fi
printf 'Fetched and verified Hysteria %s for %s.\n' "${version}" "${target}"
