#!/usr/bin/env bash
# Unit tests for packaging/installer/lib/*.sh, loaded through the installer in
# library mode. DNS and os-release inputs are stubbed; nothing touches the host.
# Variables set here are read by the sourced installer libraries.
# shellcheck disable=SC2034
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-installer-lib.XXXXXX")
trap 'rm -rf "$work"' EXIT

# shellcheck source=/dev/null
KET_INSTALLER_LIBRARY=1 . "$root/packaging/install-server.sh"
set -euo pipefail

check() {
  local label=$1
  shift
  "$@" || { printf 'FAIL: %s\n' "$label" >&2; exit 1; }
}

refute() {
  local label=$1
  shift
  if "$@"; then
    printf 'FAIL: %s\n' "$label" >&2
    exit 1
  fi
}

equals() {
  [[ $2 == "$3" ]] || { printf 'FAIL: %s: expected [%s], got [%s]\n' "$1" "$3" "$2" >&2; exit 1; }
}

# --- small helpers -----------------------------------------------------------
check 'version 2.29.1 >= 2.17' version_ge 2.29.1 2.17
check 'version 2.17 >= 2.17' version_ge 2.17 2.17
refute 'version 2.9.0 >= 2.17' version_ge 2.9.0 2.17
refute 'version 1.29 >= 2.17' version_ge 1.29 2.17
equals 'WireGuard key conversion' "$(base64url_key_to_standard 'ab-cd_ef')" 'ab+cd/ef='

# --- Cloudflare ranges ----------------------------------------------------------
for address in 104.16.1.1 172.67.200.5 131.0.72.5 173.245.63.255 162.159.0.1 188.114.97.3; do
  check "Cloudflare address $address" is_cloudflare_ipv4 "$address"
done
for address in 8.8.8.8 173.245.64.0 104.32.0.1 1.1.1.1 not-an-address; do
  refute "non-Cloudflare address $address" is_cloudflare_ipv4 "$address"
done

# --- mode detection ---------------------------------------------------------------
resolve_ipv4() {
  case $1 in
    straight.example.com) printf '8.8.8.8\n' ;;
    proxied.example.com) printf '104.16.1.1\n172.67.1.1\n' ;;
    mixed.example.com) printf '104.16.1.1\n9.9.9.9\n' ;;
    elsewhere.example.com) printf '9.9.9.9\n' ;;
    *) ;;
  esac
}

detect() {
  # Runs detection in a subshell and prints "mode domain direct_host".
  (
    mode=$1 domain=$2 direct_host=$3 public_ip=8.8.8.8
    detect_mode
    printf '%s %s %s' "$mode" "$domain" "$direct_host"
  )
}

equals 'no domain' "$(detect '' '' '')" 'auto ket-8-8-8-8.sslip.io ket-8-8-8-8.sslip.io'
equals 'domain on this server' "$(detect '' straight.example.com '')" \
  'direct straight.example.com straight.example.com'
equals 'domain behind Cloudflare' "$(detect '' proxied.example.com '')" \
  'cloudflare proxied.example.com ket-8-8-8-8.sslip.io'
equals 'Cloudflare with a named direct host' "$(detect '' proxied.example.com raw.example.com)" \
  'cloudflare proxied.example.com raw.example.com'
equals 'forced direct mode skips DNS' "$(detect direct unresolved.example.com '')" \
  'direct unresolved.example.com unresolved.example.com'
for domain in unresolved.example.com elsewhere.example.com mixed.example.com; do
  if (detect '' "$domain" '' >/dev/null 2>&1); then
    printf 'FAIL: %s should not be accepted\n' "$domain" >&2
    exit 1
  fi
done
message=$( (detect '' unresolved.example.com '') 2>&1 || true)
check 'unresolved domain explains the fix' grep -Fq 'Point an A record at 8.8.8.8' <<<"$message"

# --- operating systems ------------------------------------------------------------
os() {
  # os NAME 'KEY=value;KEY=value' -> prints "family|apt repository|dnf repository"
  printf '%s\n' "${2//;/$'\n'}" > "$work/$1"
  (
    KET_OS_RELEASE="$work/$1"
    load_os_release
    printf '%s|%s|%s' "$(os_family)" "$(apt_docker_repository)" "$(dnf_docker_repository || printf none)"
  )
}

equals 'Debian 11' "$(os debian 'ID=debian;VERSION_ID=11;VERSION_CODENAME=bullseye')" \
  'apt|debian bullseye|none'
equals 'Ubuntu 20.04' "$(os ubuntu 'ID=ubuntu;ID_LIKE=debian;VERSION_ID=20.04;VERSION_CODENAME=focal;UBUNTU_CODENAME=focal')" \
  'apt|ubuntu focal|none'
equals 'Linux Mint' "$(os mint 'ID=linuxmint;ID_LIKE="ubuntu debian";VERSION_CODENAME=wilma;UBUNTU_CODENAME=noble')" \
  'apt|ubuntu noble|none'
equals 'Raspberry Pi OS' "$(os raspberry 'ID=debian;VERSION_CODENAME=bookworm')" 'apt|debian bookworm|none'
equals 'Rocky Linux 8' "$(os rocky 'ID=rocky;ID_LIKE="rhel centos fedora";VERSION_ID=8.10')" \
  'dnf|debian |https://download.docker.com/linux/centos/docker-ce.repo'
equals 'RHEL 9' "$(os rhel 'ID=rhel;ID_LIKE=fedora;VERSION_ID=9.4')" \
  'dnf|debian |https://download.docker.com/linux/rhel/docker-ce.repo'
equals 'Fedora' "$(os fedora 'ID=fedora;VERSION_ID=41')" \
  'dnf|debian |https://download.docker.com/linux/fedora/docker-ce.repo'
equals 'Amazon Linux 2023 needs Docker preinstalled' "$(os amazon 'ID=amzn;ID_LIKE=fedora;VERSION_ID=2023')" \
  'dnf|debian |none'
equals 'openSUSE' "$(os suse 'ID=opensuse-leap;ID_LIKE="suse opensuse"')" 'zypper|debian |none'
equals 'Arch' "$(os arch 'ID=arch')" 'pacman|debian |none'
equals 'Alpine' "$(os alpine 'ID=alpine')" 'unknown|debian |none'

# --- renewal timer ----------------------------------------------------------------
(
  install_dir=/opt/ket-test
  KET_SYSTEMD_DIR=$work
  install_renewal_timer
)
check 'renewal service runs the install copy' grep -Fqx \
  'ExecStart=/opt/ket-test/packaging/server/certificates.sh renew' "$work/ket-certificates.service"
check 'renewal timer is persistent' grep -Fqx 'Persistent=true' "$work/ket-certificates.timer"

# --- generated secrets satisfy the server preflight ----------------------------------
for _ in 1 2 3 4 5 6 7 8; do
  node_key=$(random_base64url_32)
  env -i "PATH=$PATH" KET_ADMIN_TOKEN=01234567890123456789012345678901 \
    KET_PUBLIC_URL=https://ket.example.com "KET_NODE_SIGNING_KEY=$node_key" \
    "$root/packaging/validate-env.sh" >/dev/null
done

printf 'Ket installer library tests passed.\n'
