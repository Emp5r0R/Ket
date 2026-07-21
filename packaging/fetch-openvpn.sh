#!/usr/bin/env bash
set -euo pipefail

platform=${1:-}
output=${2:-}
[[ -n ${platform} && -n ${output} ]] || {
  printf 'Usage: %s <linux-amd64|linux-arm64|windows-amd64> <output>\n' "$0" >&2
  exit 2
}

version=2.7.5
source_sha256=c6864b3c7d4e059c7d6ce22d1b5fa646c8b379a06af872eeb9792b6083a44ac4
windows_sha256=20a9b2831cc3be26c250caf60891c230f3bf3e1e7bd6e17b4e182f166026377a
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
    archive="$work/openvpn.msi"
    curl --fail --location --proto '=https' --tlsv1.2 \
      --output "$archive" \
      "https://build.openvpn.net/downloads/releases/OpenVPN-${version}-I001-amd64.msi"
    printf '%s  %s\n' "$windows_sha256" "$archive" | sha256sum --check --strict -
    7z e -y -o"$work/msi" "$archive" openvpn.cab >/dev/null
    7z e -y -o"$work/files" "$work/msi/openvpn.cab" \
      bin.openvpn.exe libcrypto_3_x64.dll libssl_3_x64.dll \
      libpkcs11_helper_1.dll legacy.dll vcruntime140.dll >/dev/null
    install -d "$output"
    install -m 0755 "$work/files/bin.openvpn.exe" "$output/openvpn.exe"
    for library in libcrypto_3_x64.dll libssl_3_x64.dll libpkcs11_helper_1.dll legacy.dll vcruntime140.dll; do
      install -m 0644 "$work/files/$library" "$output/$library"
    done
    exit 0
    ;;
  *)
    printf 'Unsupported OpenVPN platform: %s\n' "$platform" >&2
    exit 2
    ;;
esac

for command in curl make pkg-config sha256sum tar; do
  command -v "$command" >/dev/null || {
    printf 'Required command is unavailable: %s\n' "$command" >&2
    exit 1
  }
done
archive="$work/openvpn-${version}.tar.gz"
curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$archive" \
  "https://github.com/OpenVPN/openvpn/releases/download/v${version}/openvpn-${version}.tar.gz"
printf '%s  %s\n' "$source_sha256" "$archive" | sha256sum --check --strict -
tar --extract --gzip --file "$archive" --directory "$work"
(
  cd "$work/openvpn-${version}"
  ./configure \
    --prefix=/usr/local \
    --disable-dco \
    --disable-lzo \
    --disable-lz4 \
    --disable-plugin-auth-pam \
    --disable-systemd
  make -j"$(getconf _NPROCESSORS_ONLN)"
)
install -D -m 0755 "$work/openvpn-${version}/src/openvpn/openvpn" "$output"
