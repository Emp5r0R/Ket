#!/usr/bin/env bash
# Install a complete Ket server on a fresh Linux VPS.
#
# Normally run through the one-command bootstrap, which downloads the latest
# release and runs this script from that release:
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash
#
# It can also run from a full source checkout: sudo ./packaging/install-server.sh
# The installer's globals are read by the sourced installer/lib/*.sh files.
# shellcheck disable=SC2034
set -euo pipefail

installer_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# Shared with the ket command: host paths, timers, the command link, and the
# server image. Loaded first so the installer's own helpers (fail) take precedence.
for library in paths common units image; do
  # shellcheck source=/dev/null
  . "$installer_dir/server/cli/$library.sh"
done
for library in common validate network system certificates provision report; do
  # shellcheck source=/dev/null
  . "$installer_dir/installer/lib/$library.sh"
done

readonly REPOSITORY=Emp5r0R/Ket

usage() {
  cat <<'EOF'
Install a Ket server.

Usage: install-server.sh [DOMAIN] [options]

  DOMAIN               Optional. Without it, Ket uses an automatic address
                       (ket-<ip>.sslip.io) and needs no DNS. With it, Ket checks
                       where DOMAIN points: straight at this server, or through
                       Cloudflare (then raw transports use the automatic address
                       unless --direct-host is given).

Options:
  --email EMAIL        Let's Encrypt expiry notices (optional)
  --direct-host HOST   DNS-only name for raw transports when behind Cloudflare
  --mode MODE          Force auto, direct, or cloudflare instead of detecting
  --public-ip IPv4     Override public IPv4 detection
  --max-sessions N     Concurrent sessions, 1-512 (default 32)
  --first-code-valid-minutes N
                       Lifetime of the first invite, 1-525600 (default 43200)
  --country-code XX --country-name NAME --city NAME --latitude N --longitude N
                       Map location, all five together (default: detected)
  --install-dir PATH   Installation directory (default /opt/ket)
  --auto-update POLICY Which releases the daily check applies on its own:
                       off, critical, security (default), or all
  --plan               Show what would be set up, change nothing
  --help               Show this help
EOF
}

mode=''
domain=''
direct_host=''
email=''
public_ip=''
country_code='' country_name='' city='' latitude='' longitude=''
location_fields=0
max_sessions=32
first_code_valid_minutes=43200
git_ref=''
source_dir=''
install_dir=/opt/ket
plan=false
wireguard_enabled=true
auto_update=security

parse_arguments() {
  while (($#)); do
    case $1 in
      --domain) need_value "$@"; domain=$2; shift 2 ;;
      --direct-host) need_value "$@"; direct_host=$2; shift 2 ;;
      --email) need_value "$@"; email=$2; shift 2 ;;
      --mode) need_value "$@"; mode=$2; shift 2 ;;
      --public-ip) need_value "$@"; public_ip=$2; shift 2 ;;
      --country-code) need_value "$@"; country_code=${2^^}; location_fields=$((location_fields + 1)); shift 2 ;;
      --country-name) need_value "$@"; country_name=$2; location_fields=$((location_fields + 1)); shift 2 ;;
      --city) need_value "$@"; city=$2; location_fields=$((location_fields + 1)); shift 2 ;;
      --latitude) need_value "$@"; latitude=$2; location_fields=$((location_fields + 1)); shift 2 ;;
      --longitude) need_value "$@"; longitude=$2; location_fields=$((location_fields + 1)); shift 2 ;;
      --max-sessions) need_value "$@"; max_sessions=$2; shift 2 ;;
      --first-code-valid-minutes) need_value "$@"; first_code_valid_minutes=$2; shift 2 ;;
      --ref) need_value "$@"; git_ref=$2; shift 2 ;;
      --source-dir) need_value "$@"; source_dir=$2; shift 2 ;;
      --install-dir) need_value "$@"; install_dir=$2; shift 2 ;;
      --auto-update) need_value "$@"; auto_update=$2; shift 2 ;;
      --plan) plan=true; shift ;;
      --help|-h) usage; exit 0 ;;
      -*) fail "unknown option: $1 (see --help)" ;;
      *)
        [[ -z $domain ]] || fail "unexpected argument: $1"
        domain=$1
        shift
        ;;
    esac
  done
}

validate_arguments() {
  [[ -z $email ]] || valid_email "$email" || fail '--email is invalid'
  [[ -z $public_ip ]] || valid_public_ipv4 "$public_ip" \
    || fail '--public-ip must be a publicly routable IPv4 address'
  [[ -z $domain ]] || valid_hostname "$domain" || fail "$domain is not a valid hostname"
  [[ -z $direct_host ]] || valid_hostname "$direct_host" || fail '--direct-host is not a valid hostname'
  ((location_fields == 0 || location_fields == 5)) \
    || fail 'give all five location options together, or none for automatic detection'
  ((location_fields == 0)) || validate_location
  if [[ ! $max_sessions =~ ^[1-9][0-9]*$ ]] || ((max_sessions > 512)); then
    fail '--max-sessions must be between 1 and 512'
  fi
  if [[ ! $first_code_valid_minutes =~ ^[1-9][0-9]*$ ]] || ((first_code_valid_minutes > 525600)); then
    fail '--first-code-valid-minutes must be between 1 and 525600'
  fi
  [[ -z $git_ref || ($git_ref =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ && $git_ref != *..*) ]] \
    || fail '--ref is invalid'
  [[ $install_dir =~ ^/[A-Za-z0-9._/-]{1,240}$ && $install_dir != / \
    && $install_dir != *..* && $install_dir != *//* ]] \
    || fail '--install-dir must be a safe absolute path without spaces or parent traversal'
  case $auto_update in
    off|critical|security|all) ;;
    *) fail '--auto-update must be off, critical, security, or all' ;;
  esac
  shadowsocks_start=20000
  shadowsocks_end=$((shadowsocks_start + max_sessions - 1))
  first_code_max_connections=$((max_sessions < 5 ? max_sessions : 5))
  client_release_url="https://github.com/$REPOSITORY/releases/latest"
  [[ ! $git_ref =~ ^v[0-9] ]] || client_release_url="https://github.com/$REPOSITORY/releases/tag/$git_ref"
}

check_ports_free() {
  local port
  for port in 80 443 9443; do
    ! listener_uses_port tcp "$port" || fail "TCP port $port is already in use"
  done
  ! listener_uses_port udp 443 || fail 'UDP port 443 is already in use'
  ! listener_uses_port tcp "$shadowsocks_start" "$shadowsocks_end" \
    || fail "a TCP port in $shadowsocks_start-$shadowsocks_end is already in use"
  ! listener_uses_port udp "$shadowsocks_start" "$shadowsocks_end" \
    || fail "a UDP port in $shadowsocks_start-$shadowsocks_end is already in use"
}

check_direct_address() {
  local resolved
  resolved=$(resolve_ipv4 "$direct_host")
  grep -Fqx -- "$public_ip" <<<"$resolved" \
    || fail "$direct_host must point to $public_ip (it resolves to: ${resolved:-nothing})"
}

# Stages the release tree to install: the bootstrap's download (used in
# place), or a copy of the checkout this script runs from without build output.
# The staging directory becomes install_dir and is removed on failure.
stage_source() {
  local parent
  parent=$(dirname -- "$install_dir")
  install -d -m 0755 "$parent"
  if [[ -n $source_dir ]]; then
    staging_dir=$source_dir
  else
    staging_dir=$(mktemp -d "$parent/.ket-install.XXXXXX")
    tar -C "$installer_dir/.." \
      --exclude=./.git --exclude=./target --exclude='*/node_modules' \
      --exclude=./build-outputs --exclude=./release-artifacts --exclude=./.env \
      -cf - . | tar -C "$staging_dir" -xf -
  fi
  trap 'rm -rf -- "${staging_dir:-}"' EXIT
  source_dir=$staging_dir
}

finish_installation() {
  printf '%s\n' "${git_ref:-source}" > "$staging_dir/.ket-release"
  cd "$(dirname -- "$install_dir")"
  mv -- "$staging_dir" "$install_dir"
  staging_dir=''
  trap - EXIT
  cd "$install_dir"
  ./packaging/server/compose.sh up --detach --no-build --remove-orphans
  install_host_integration
  wait_until_ready
  printf '\nKet is ready at https://%s\n' "$domain"
  ./packaging/server/create-invite.sh \
    --label 'First devices' \
    --devices "$first_code_max_connections" \
    --minutes "$first_code_valid_minutes" \
    || fail 'Ket is running, but the first invite could not be created; run: sudo ket invite'
  print_handoff
}

main() {
  local family arch
  parse_arguments "$@"
  validate_arguments
  if [[ -z $public_ip ]]; then
    command -v curl >/dev/null 2>&1 || fail 'curl is required to detect the public IPv4 address'
    detect_public_ipv4 || fail 'could not detect a public IPv4 address; rerun with --public-ip'
  fi
  detect_mode
  if $plan; then
    print_plan
    printf 'Install directory: %s\n' "$install_dir"
    printf 'First invite: up to %s device(s) for %s minutes\n' "$first_code_max_connections" "$first_code_valid_minutes"
    printf 'Automatic updates: %s\n' "$auto_update"
    exit 0
  fi

  [[ $EUID -eq 0 ]] || fail 'run as root (curl ... | sudo bash)'
  [[ ! -e $install_dir ]] || fail "$install_dir already exists; to update it, run: sudo ket update"
  require_systemd
  load_os_release
  family=$(os_family)
  arch=$(cpu_arch)
  ensure_base_tools "$family"
  print_plan
  check_direct_address
  check_ports_free
  apply_host_firewall_rules
  if ((location_fields == 0)); then
    detect_location || fail 'could not detect the server location; rerun with all five location options'
    validate_location
    printf 'Detected server location: %s, %s\n' "$city" "$country_name"
  fi
  ensure_docker "$family" "$arch"
  kernel_supports_wireguard || wireguard_enabled=false
  stage_source
  issue_certificate
  cd "$staging_dir"
  umask 077
  write_environment_file
  create_openvpn_pki
  build_and_validate
  finish_installation
}

[[ -n ${KET_INSTALLER_LIBRARY:-} ]] || main "$@"
