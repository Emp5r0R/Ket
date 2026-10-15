# shellcheck shell=bash
# Network discovery and mode detection for the Ket server installer.
# Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

# Cloudflare's published edge ranges (https://www.cloudflare.com/ips-v4).
readonly CLOUDFLARE_IPV4_RANGES=(
  173.245.48.0/20 103.21.244.0/22 103.22.200.0/22 103.31.4.0/22
  141.101.64.0/18 108.162.192.0/18 190.93.240.0/20 188.114.96.0/20
  197.234.240.0/22 198.41.128.0/17 162.158.0.0/15 104.16.0.0/13
  104.24.0.0/14 172.64.0.0/13 131.0.72.0/22
)

detect_public_ipv4() {
  local endpoint candidate
  for endpoint in https://api.ipify.org https://ipv4.icanhazip.com; do
    candidate=$(curl -4fsS --connect-timeout 5 --max-time 10 "$endpoint" 2>/dev/null | tr -d '[:space:]') || continue
    if valid_public_ipv4 "$candidate"; then
      public_ip=$candidate
      return 0
    fi
  done
  return 1
}

automatic_hostname() {
  printf 'ket-%s.sslip.io' "${1//./-}"
}

# Prints the IPv4 addresses a name resolves to, one per line. Tests replace it.
# resolve_ipv4 NAME: the name's IPv4 addresses, one per line; nothing (and
# success) when it does not resolve, so callers can explain what to fix.
resolve_ipv4() {
  { getent ahostsv4 "$1" 2>/dev/null || true; } | awk '{print $1}' | LC_ALL=C sort -u
}

ipv4_to_int() {
  local IFS=.
  local -a octets
  read -r -a octets <<<"$1"
  printf '%s' $(((octets[0] << 24) | (octets[1] << 16) | (octets[2] << 8) | octets[3]))
}

ipv4_in_cidr() {
  local bits=${2#*/} mask
  mask=$(((0xffffffff << (32 - bits)) & 0xffffffff))
  (( ($(ipv4_to_int "$1") & mask) == ($(ipv4_to_int "${2%/*}") & mask) ))
}

is_cloudflare_ipv4() {
  local range
  [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1
  for range in "${CLOUDFLARE_IPV4_RANGES[@]}"; do
    ipv4_in_cidr "$1" "$range" && return 0
  done
  return 1
}

# Resolves mode, domain, and direct_host from what the operator supplied.
# Inputs (globals): mode (auto|direct|cloudflare or empty), domain,
# direct_host, public_ip. No domain means automatic sslip.io mode; a domain is
# classified by where it resolves: this server (direct) or Cloudflare.
detect_mode() {
  local addresses address all_cloudflare=true
  if [[ -z $domain ]]; then
    [[ -z $mode || $mode == auto ]] || fail "--mode $mode needs a domain"
    [[ -z $direct_host ]] || fail '--direct-host needs a domain'
    mode=auto
    domain=$(automatic_hostname "$public_ip")
    direct_host=$domain
    return 0
  fi
  [[ $mode != auto ]] || fail 'automatic mode does not take a domain'
  if [[ -z $mode ]]; then
    addresses=$(resolve_ipv4 "$domain")
    [[ -n $addresses ]] || fail "$domain does not resolve yet. Point an A record at $public_ip (directly or through Cloudflare), wait for DNS, and rerun."
    if grep -Fqx -- "$public_ip" <<<"$addresses"; then
      mode=direct
    else
      while IFS= read -r address; do
        is_cloudflare_ipv4 "$address" || all_cloudflare=false
      done <<<"$addresses"
      $all_cloudflare || fail "$domain resolves to $(tr '\n' ' ' <<<"$addresses")which is neither this server ($public_ip) nor Cloudflare"
      mode=cloudflare
    fi
  fi
  case $mode in
    direct)
      [[ -z $direct_host || $direct_host == "$domain" ]] || fail '--direct-host is only used behind Cloudflare'
      direct_host=$domain
      ;;
    cloudflare)
      # Raw UDP and TCP transports cannot pass through Cloudflare's proxy; an
      # automatic public-IP hostname needs no extra DNS record.
      [[ -n $direct_host ]] || direct_host=$(automatic_hostname "$public_ip")
      [[ $direct_host != "$domain" ]] || fail 'behind Cloudflare, --direct-host must be a separate DNS-only name'
      ;;
    *) fail '--mode must be auto, direct, or cloudflare' ;;
  esac
}

listener_uses_port() {
  local socket_type=$1 requested_start=$2 requested_end=${3:-$2}
  local flag=-ltn sockets
  command -v ss >/dev/null 2>&1 || return 1
  [[ $socket_type == udp ]] && flag=-lun
  sockets=$(ss -H "$flag" 2>/dev/null) || fail "could not inspect $socket_type listeners with ss"
  awk '{ value=$4; sub(/^.*:/, "", value); print value }' <<<"$sockets" \
    | awk -v first="$requested_start" -v last="$requested_end" \
        '$1 ~ /^[0-9]+$/ && $1 >= first && $1 <= last { found=1 } END { exit !found }'
}

parse_ipwho_location() {
  local response=$1
  [[ $(jq -r '.success // false' <<<"$response") == true ]] || return 1
  country_code=$(jq -er '.country_code | select(type == "string" and length == 2) | ascii_upcase' <<<"$response") || return 1
  country_name=$(jq -er '.country | select(type == "string" and length > 0)' <<<"$response") || return 1
  city=$(jq -er '.city | select(type == "string" and length > 0)' <<<"$response") || return 1
  latitude=$(jq -er '.latitude | select(type == "number")' <<<"$response") || return 1
  longitude=$(jq -er '.longitude | select(type == "number")' <<<"$response") || return 1
}

parse_ipapi_location() {
  local response=$1
  country_code=$(jq -er '.country_code | select(type == "string" and length == 2) | ascii_upcase' <<<"$response") || return 1
  country_name=$(jq -er '.country_name | select(type == "string" and length > 0)' <<<"$response") || return 1
  city=$(jq -er '.city | select(type == "string" and length > 0)' <<<"$response") || return 1
  latitude=$(jq -er '.latitude | select(type == "number")' <<<"$response") || return 1
  longitude=$(jq -er '.longitude | select(type == "number")' <<<"$response") || return 1
}

detect_location() {
  local response
  if response=$(curl -fsS --connect-timeout 5 --max-time 10 https://ipwho.is/ 2>/dev/null) \
    && parse_ipwho_location "$response"; then
    return 0
  fi
  if response=$(curl -fsS --connect-timeout 5 --max-time 10 https://ipapi.co/json/ 2>/dev/null) \
    && parse_ipapi_location "$response"; then
    return 0
  fi
  return 1
}
