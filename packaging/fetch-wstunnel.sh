#!/usr/bin/env bash
set -euo pipefail

version=10.6.2
target=${1:-}
destination=${2:-}

if [[ -z ${target} || -z ${destination} ]]; then
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64|android-arm64> <destination>\n' "$0" >&2
  exit 2
fi

case "${target}" in
  linux-amd64)
    asset=wstunnel_${version}_linux_amd64.tar.gz
    executable=wstunnel
    checksum=db6064cca0515b67f8652e201cff8e27553b8cbb7216b2e19241311e34868e6e
    ;;
  linux-arm64)
    asset=wstunnel_${version}_linux_arm64.tar.gz
    executable=wstunnel
    checksum=26bb36b856948255bec7cd71a39df5f8912acdd7a47a9ccd4044a9b80ced108d
    ;;
  windows-amd64)
    asset=wstunnel_${version}_windows_amd64.tar.gz
    executable=wstunnel.exe
    checksum=3a88e9533845fdd377c79f5aa61f9eb7cedbfc26639533e7b944e317408f1c3b
    ;;
  android-arm64)
    asset=wstunnel_${version}_android_arm64.tar.gz
    executable=wstunnel
    checksum=f1a754142cb79b6422e72d1bf7b5767a04d604f988abe9f517fb33a7b5c3b46d
    ;;
  *)
    printf 'Unsupported wstunnel target: %s\n' "${target}" >&2
    exit 2
    ;;
esac

archive=$(mktemp)
stage=$(mktemp -d)
cleanup() {
  rm -f "$archive"
  rm -rf "$stage"
}
trap cleanup EXIT
url="https://github.com/erebe/wstunnel/releases/download/v${version}/${asset}"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --output "$archive" "$url"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
tar -xzf "$archive" -C "$stage" "$executable"
mkdir -p "$(dirname "$destination")"
cp "$stage/$executable" "$destination"
chmod 0755 "$destination" 2>/dev/null || true
printf 'Fetched and verified wstunnel %s for %s.\n' "$version" "$target"
