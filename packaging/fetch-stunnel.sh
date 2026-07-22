#!/usr/bin/env bash
set -euo pipefail

platform=${1:-}
output=${2:-}
[[ -n ${platform} && -n ${output} ]] || {
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64> <output>\n' "$0" >&2
  exit 2
}

version=5.79
source_sha256=8ea0de6e5ea76f38ea987fa831c7fd47f7a1f1e7dd465fd6fa8622edf30d3a45
windows_sha256=3d946e9759d5a6c6fc5aadb6b56e7de1a81d210ad028235340616e471afeb6f6
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

case ${platform} in
  linux-amd64)
    [[ $(uname -m) == x86_64 ]] || {
      printf 'linux-amd64 must be built natively on x86_64.\n' >&2
      exit 1
    }
    ;;
  linux-arm64)
    [[ $(uname -m) == aarch64 || $(uname -m) == arm64 ]] || {
      printf 'linux-arm64 must be built natively on arm64.\n' >&2
      exit 1
    }
    ;;
  windows-amd64)
    for command in curl sha256sum 7z; do
      command -v "$command" >/dev/null || {
        printf 'Required command is unavailable: %s\n' "$command" >&2
        exit 1
      }
    done
    archive="$work/stunnel-installer.exe"
    curl --fail --location --proto '=https' --tlsv1.2 \
      --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
      --speed-limit 1024 --speed-time 30 \
      --output "$archive" \
      "https://www.stunnel.org/downloads/stunnel-${version}-win64-installer.exe"
    printf '%s  %s\n' "$windows_sha256" "$archive" | sha256sum --check --strict -
    (
      cd "$work"
      7z x -y -ofiles stunnel-installer.exe >/dev/null
    )
    install -d "$output/ossl-modules"
    install -m 0755 "$work/files/bin/stunnel.exe" "$output/stunnel.exe"
    for library in libssp-0.dll libcrypto-3-x64.dll libssl-3-x64.dll; do
      install -m 0644 "$work/files/bin/$library" "$output/$library"
    done
    install -m 0644 "$work/files/ossl-modules/legacy.dll" "$output/ossl-modules/legacy.dll"
    exit 0
    ;;
  *)
    printf 'Unsupported stunnel platform: %s\n' "$platform" >&2
    exit 2
    ;;
esac

if [[ -d "$output" ]]; then
  printf 'Linux stunnel output must be a file, but a directory exists: %s\n' "$output" >&2
  exit 1
fi

for command in curl make pkg-config sha256sum tar; do
  command -v "$command" >/dev/null || {
    printf 'Required command is unavailable: %s\n' "$command" >&2
    exit 1
  }
done
archive="$work/stunnel-${version}.tar.gz"
curl --fail --location --proto '=https' --tlsv1.2 \
  --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
  --speed-limit 1024 --speed-time 30 \
  --output "$archive" \
  "https://www.stunnel.org/downloads/stunnel-${version}.tar.gz"
printf '%s  %s\n' "$source_sha256" "$archive" | sha256sum --check --strict -
tar --extract --gzip --file "$archive" --directory "$work"
(
  cd "$work/stunnel-${version}"
  ./configure --prefix=/usr/local --disable-libwrap
  make -j"$(getconf _NPROCESSORS_ONLN)"
)
install -D -m 0755 "$work/stunnel-${version}/src/stunnel" "$output"
