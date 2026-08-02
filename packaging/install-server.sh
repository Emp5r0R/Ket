#!/usr/bin/env bash
set -euo pipefail

readonly REPOSITORY_URL=https://github.com/Emp5r0R/Ket.git
readonly XRAY_IMAGE='ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7'
readonly DEFAULT_GIT_REF=v1.0.0
readonly CLIENT_RELEASE_URL=https://github.com/Emp5r0R/Ket/releases/tag/v1.0.0

usage() {
  cat <<'EOF'
Install a complete Ket server on a fresh Debian 12+ or Ubuntu 22.04+ VPS.

Usage:
  install-server.sh --email EMAIL [options]

Required:
  --email EMAIL          Let's Encrypt account email

Modes:
  --mode auto            No domain; use an automatic public-IP sslip.io hostname
                         (default when --domain is omitted)
  --mode direct          Your domain resolves directly to this VPS
  --mode cloudflare      HOST is Cloudflare-proxied; --direct-host is DNS-only
  --domain HOST          Required in direct and Cloudflare modes
  --direct-host HOST     Raw Hysteria/REALITY/Shadowsocks/OpenVPN hostname
  --public-ip IPv4       Override automatic public IPv4 detection

Location and capacity:
  Location is detected from the VPS public IP by default. To override it,
  provide all five location options together:
  --country-code XX      Two-letter country code
  --country-name NAME    Display country
  --city NAME            Display city
  --latitude NUMBER      Map latitude
  --longitude NUMBER     Map longitude
  --max-sessions N       Maximum concurrent sessions, 1-512 (default: 32)
  --first-code-valid-minutes N
                         First access-code lifetime, 1-525600 (default: 43200)

Other:
  --ref REF              Git tag or branch to install (default: v1.0.0)
  --install-dir PATH     Installation directory (default: /opt/ket)
  --plan                 Validate and print the required ingress without changes
  --help                 Show this help

Automatic mode needs no DNS account. It uses <public-ip>.sslip.io, an external
wildcard DNS service. A domain you control is preferred for long-lived service.

Cloudflare mode requires two DNS records before installation:
  1. --domain: proxied (orange cloud)
  2. --direct-host: DNS-only (gray cloud), pointing to the same VPS
EOF
}

fail() { printf 'Ket installer error: %s\n' "$1" >&2; exit 1; }
need_value() { [[ $# -ge 2 && -n $2 ]] || fail "$1 requires a value"; }
valid_hostname() {
  local hostname=$1 label
  local IFS=.
  local -a labels
  [[ ${#hostname} -le 253 && $hostname == *.* && $hostname != *..* \
    && $hostname != .* && $hostname != *. ]] || return 1
  read -r -a labels <<<"$hostname"
  for label in "${labels[@]}"; do
    [[ ${#label} -ge 1 && ${#label} -le 63 ]] || return 1
    [[ $label =~ ^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$ ]] || return 1
  done
}
valid_display_text() {
  [[ -n $1 && ${#1} -le 64 ]] || return 1
  [[ $1 != [[:space:]]* && $1 != *[[:space:]] ]] || return 1
  [[ ! $1 =~ [[:cntrl:]] ]] || return 1
  [[ $1 != *'"'* && $1 != *'$'* && $1 != *'`'* ]]
  [[ $1 != *\\* ]]
}
valid_public_ipv4() {
  local value=$1 octet
  local IFS=.
  local -a octets
  [[ $value =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1
  read -r -a octets <<<"$value"
  [[ ${#octets[@]} -eq 4 ]] || return 1
  for octet in "${octets[@]}"; do
    [[ $octet =~ ^(0|[1-9][0-9]{0,2})$ ]] || return 1
    ((octet <= 255)) || return 1
  done
  ((octets[0] != 0 && octets[0] != 10 && octets[0] != 127 && octets[0] < 224)) || return 1
  ((!(octets[0] == 100 && octets[1] >= 64 && octets[1] <= 127))) || return 1
  ((!(octets[0] == 169 && octets[1] == 254))) || return 1
  ((!(octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31))) || return 1
  ((!(octets[0] == 192 && (octets[1] == 168 || (octets[1] == 0 && (octets[2] == 0 || octets[2] == 2)))))) || return 1
  ((!(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19 || (octets[1] == 51 && octets[2] == 100))))) || return 1
  ((!(octets[0] == 203 && octets[1] == 0 && octets[2] == 113))) || return 1
}
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
print_ingress() {
  printf '  TCP 80                 ACME certificate issuance and renewal\n'
  printf '  TCP 443                Control, REALITY, HTTPS Stealth, WireGuard TLS\n'
  printf '  UDP 443                Hysteria2 and Hysteria XOR\n'
  printf '  TCP 9443               OpenVPN over TLS\n'
  printf '  TCP+UDP %s-%s     Shadowsocks 2022 lease ports\n' "$shadowsocks_start" "$shadowsocks_end"
}
print_operator_checklist() {
  printf '\n=== Ket VPS checklist ===\n'
  printf 'Mode: %s\nControl hostname: %s\nRaw transport hostname: %s\n' "$mode" "$domain" "$direct_host"
  if [[ -n $public_ip ]]; then
    printf 'Expected VPS public IPv4: %s\n' "$public_ip"
  fi
  printf '\nOpen these STATEFUL inbound rules in your cloud firewall/security group:\n'
  print_ingress
  printf '  Source: 0.0.0.0/0\n'
  printf '\nKeep your existing SSH rule. Do NOT expose TCP 8787, 8445, or 8446; they remain loopback/private origins.\n'
  case "$mode" in
    auto)
      printf 'DNS: none to configure. Ket will use the automatic hostname above.\n'
      ;;
    direct)
      printf 'DNS: create an A record for %s pointing to %s. Do not proxy it.\n' "$domain" "${public_ip:-this VPS public IPv4}"
      ;;
    cloudflare)
      printf 'Cloudflare DNS: %s = Proxied; %s = DNS only. Both point to %s.\n' \
        "$domain" "$direct_host" "${public_ip:-this VPS public IPv4}"
      printf 'Cloudflare SSL/TLS: Full (strict). Keep WebSockets enabled.\n'
      ;;
  esac
  printf 'TCP 80 must be reachable before Certbot runs. Configure provider ingress before installing.\n'
  printf '=========================\n\n'
}
listener_uses_port() {
  local socket_type=$1 requested_start=$2 requested_end=${3:-$2}
  local flag=-ltn sockets
  [[ $socket_type == udp ]] && flag=-lun
  sockets=$(ss -H "$flag" 2>/dev/null) || fail "could not inspect $socket_type listeners with ss"
  awk '{ value=$4; sub(/^.*:/, "", value); print value }' <<<"$sockets" \
    | awk -v first="$requested_start" -v last="$requested_end" \
        '$1 ~ /^[0-9]+$/ && $1 >= first && $1 <= last { found=1 } END { exit !found }'
}
apply_host_firewall_rules() {
  if command -v ufw >/dev/null 2>&1 && ufw status | grep -q '^Status: active'; then
    printf 'Adding Ket rules to active UFW firewall...\n'
    ufw allow 80/tcp comment 'Ket ACME renewal'
    ufw allow 443/tcp comment 'Ket HTTPS transports'
    ufw allow 443/udp comment 'Ket Hysteria2'
    ufw allow 9443/tcp comment 'Ket OpenVPN stunnel'
    ufw allow "$shadowsocks_start:$shadowsocks_end/tcp" comment 'Ket Shadowsocks TCP'
    ufw allow "$shadowsocks_start:$shadowsocks_end/udp" comment 'Ket Shadowsocks UDP'
  else
    printf 'UFW is absent or inactive; no host-firewall policy was changed. Provider firewall rules are still required.\n'
  fi
}
validate_location() {
  [[ $country_code =~ ^[A-Z]{2}$ ]] || fail '--country-code must contain two letters'
  valid_display_text "$country_name" || fail '--country-name must be 1-64 safe printable characters'
  valid_display_text "$city" || fail '--city must be 1-64 safe printable characters'
  [[ $latitude =~ ^-?[0-9]+([.][0-9]+)?$ ]] || fail '--latitude must be numeric'
  [[ $longitude =~ ^-?[0-9]+([.][0-9]+)?$ ]] || fail '--longitude must be numeric'
  awk -v value="$latitude" 'BEGIN { exit !(value >= -90 && value <= 90) }' \
    || fail '--latitude must be between -90 and 90'
  awk -v value="$longitude" 'BEGIN { exit !(value >= -180 && value <= 180) }' \
    || fail '--longitude must be between -180 and 180'
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
random_secret() { openssl rand -base64 48 | tr -d '\n'; }
random_hex() { openssl rand -hex "$1"; }

mode=auto
mode_set=false
domain=
direct_host=
email=
public_ip=
country_code=
country_name=
city=
latitude=
longitude=
country_code_set=false
country_name_set=false
city_set=false
latitude_set=false
longitude_set=false
max_sessions=32
first_code_valid_minutes=43200
git_ref=$DEFAULT_GIT_REF
install_dir=/opt/ket
plan=false

while (($#)); do
  case "$1" in
    --domain) need_value "$@"; domain=$2; shift 2 ;;
    --direct-host) need_value "$@"; direct_host=$2; shift 2 ;;
    --email) need_value "$@"; email=$2; shift 2 ;;
    --mode) need_value "$@"; mode=$2; mode_set=true; shift 2 ;;
    --public-ip) need_value "$@"; public_ip=$2; shift 2 ;;
    --country-code) need_value "$@"; country_code=${2^^}; country_code_set=true; shift 2 ;;
    --country-name) need_value "$@"; country_name=$2; country_name_set=true; shift 2 ;;
    --city) need_value "$@"; city=$2; city_set=true; shift 2 ;;
    --latitude) need_value "$@"; latitude=$2; latitude_set=true; shift 2 ;;
    --longitude) need_value "$@"; longitude=$2; longitude_set=true; shift 2 ;;
    --max-sessions) need_value "$@"; max_sessions=$2; shift 2 ;;
    --first-code-valid-minutes) need_value "$@"; first_code_valid_minutes=$2; shift 2 ;;
    --ref) need_value "$@"; git_ref=$2; shift 2 ;;
    --install-dir) need_value "$@"; install_dir=$2; shift 2 ;;
    --plan) plan=true; shift ;;
    --help|-h) usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[[ -n $email ]] || fail '--email is required'
[[ $email =~ ^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$ ]] || fail '--email is invalid'
case "$mode" in auto|direct|cloudflare) ;; *) fail '--mode must be auto, direct, or cloudflare' ;; esac
if [[ $mode_set == false && -n $domain ]]; then
  mode=direct
fi
if [[ -n $public_ip ]]; then
  valid_public_ipv4 "$public_ip" || fail '--public-ip must be a publicly routable IPv4 address'
fi

case "$mode" in
  auto)
    [[ -z $domain && -z $direct_host ]] || fail 'auto mode does not accept --domain or --direct-host'
    command -v curl >/dev/null 2>&1 \
      || fail 'curl is required; install it with: apt-get update && apt-get install -y curl'
    [[ -n $public_ip ]] || detect_public_ipv4 \
      || fail 'could not detect a public IPv4 address; use --public-ip or configure your own domain'
    domain=$(automatic_hostname "$public_ip")
    direct_host=$domain
    ;;
  direct)
    [[ -n $domain ]] || fail '--domain is required in direct mode'
    [[ -z $direct_host ]] || fail '--direct-host is only used in cloudflare mode'
    direct_host=$domain
    ;;
  cloudflare)
    [[ -n $domain ]] || fail '--domain is required in cloudflare mode'
    [[ -n $direct_host ]] || fail '--direct-host is required in cloudflare mode'
    [[ $direct_host != "$domain" ]] || fail 'cloudflare mode requires a separate DNS-only --direct-host'
    ;;
esac
valid_hostname "$domain" || fail '--domain must be a valid hostname'
valid_hostname "$direct_host" || fail '--direct-host must be a valid hostname'
location_fields=0
for supplied in "$country_code_set" "$country_name_set" "$city_set" "$latitude_set" "$longitude_set"; do
  $supplied && location_fields=$((location_fields + 1))
done
if ((location_fields != 0 && location_fields != 5)); then
  fail 'provide all five location options together, or omit all five for automatic detection'
fi
location_mode=automatic
if ((location_fields == 5)); then
  location_mode=manual
  validate_location
fi
if [[ ! $max_sessions =~ ^[1-9][0-9]*$ ]] || ((max_sessions > 512)); then
  fail '--max-sessions must be between 1 and 512'
fi
if [[ ! $first_code_valid_minutes =~ ^[1-9][0-9]*$ ]] || ((first_code_valid_minutes > 525600)); then
  fail '--first-code-valid-minutes must be between 1 and 525600'
fi
[[ $git_ref =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ && $git_ref != *..* ]] \
  || fail '--ref is invalid'
[[ $install_dir =~ ^/[A-Za-z0-9._/-]{1,240}$ && $install_dir != / \
  && $install_dir != *..* && $install_dir != *//* ]] \
  || fail '--install-dir must be a safe absolute path without spaces or parent traversal'

shadowsocks_start=20000
shadowsocks_end=$((shadowsocks_start + max_sessions - 1))
first_code_max_connections=$((max_sessions < 5 ? max_sessions : 5))

if $plan; then
  print_operator_checklist
  printf 'Required ingress: TCP 80,443,9443,%s-%s; UDP 443,%s-%s\n' \
    "$shadowsocks_start" "$shadowsocks_end" "$shadowsocks_start" "$shadowsocks_end"
  if [[ $location_mode == automatic ]]; then
    printf 'Location: automatic public-IP detection\n'
  else
    printf 'Location: %s, %s (%s; %s,%s)\n' "$city" "$country_name" "$country_code" "$latitude" "$longitude"
  fi
  printf 'Install directory: %s\nGit ref: %s\n' "$install_dir" "$git_ref"
  printf 'First access-code concurrent-connection limit: %s\n' "$first_code_max_connections"
  printf 'First access-code lifetime: %s minutes\n' "$first_code_valid_minutes"
  exit 0
fi

[[ $EUID -eq 0 ]] || fail 'run the installer as root (curl ... | sudo bash -s -- ...)'
[[ ! -e $install_dir ]] || fail "$install_dir already exists; keep its .env and use the documented upgrade procedure"

if [[ ! -r /etc/os-release ]]; then
  fail 'cannot detect the Linux distribution'
fi
# The distribution file is validated as readable immediately above.
# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in debian|ubuntu) ;; *) fail 'only Debian and Ubuntu are currently supported' ;; esac
case "$ID" in
  debian) dpkg --compare-versions "${VERSION_ID:-0}" ge 12 || fail 'Debian 12 or newer is required' ;;
  ubuntu) dpkg --compare-versions "${VERSION_ID:-0}" ge 22.04 || fail 'Ubuntu 22.04 or newer is required' ;;
esac
arch=$(dpkg --print-architecture)
case "$arch" in amd64|arm64) ;; *) fail 'only amd64 and arm64 servers are supported' ;; esac

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y ca-certificates certbot curl git gnupg iproute2 jq openssl wireguard-tools

if [[ -z $public_ip ]]; then
  detect_public_ipv4 || fail 'could not detect the VPS public IPv4 address; rerun with --public-ip'
fi
print_operator_checklist

resolved_direct=$(getent ahostsv4 "$direct_host" 2>/dev/null | awk '{print $1}' | LC_ALL=C sort -u || true)
if ! grep -Fqx -- "$public_ip" <<<"$resolved_direct"; then
  fail "$direct_host must resolve directly to $public_ip before installation (resolved: ${resolved_direct:-nothing})"
fi
if [[ $mode == cloudflare ]] && ! getent ahosts "$domain" >/dev/null 2>&1; then
  fail "$domain does not resolve; create and propagate the proxied Cloudflare DNS record first"
fi

for port in 80 443 9443; do
  listener_uses_port tcp "$port" && fail "TCP port $port is already in use"
done
listener_uses_port udp 443 && fail 'UDP port 443 is already in use'
listener_uses_port tcp "$shadowsocks_start" "$shadowsocks_end" \
  && fail "a TCP port in $shadowsocks_start-$shadowsocks_end is already in use"
listener_uses_port udp "$shadowsocks_start" "$shadowsocks_end" \
  && fail "a UDP port in $shadowsocks_start-$shadowsocks_end is already in use"

apply_host_firewall_rules

if [[ $location_mode == automatic ]]; then
  detect_location || fail 'could not detect VPS location; rerun with all five location options'
  validate_location
  printf 'Detected VPS location: %s, %s (%s; %s,%s)\n' \
    "$city" "$country_name" "$country_code" "$latitude" "$longitude"
fi

certbot_args=(
  certonly --standalone --non-interactive --agree-tos --no-eff-email
  --preferred-challenges http --email "$email" --cert-name "$domain" -d "$domain"
)
if [[ $direct_host != "$domain" ]]; then
  certbot_args+=(-d "$direct_host")
fi
printf 'Requesting the TLS certificate after DNS and listener preflight...\n'
certbot "${certbot_args[@]}"

if ! docker compose version >/dev/null 2>&1; then
  if command -v docker >/dev/null 2>&1; then
    fail 'Docker is installed without Compose v2; install docker-compose-plugin, then rerun'
  fi
  conflicting_packages=()
  for package in docker.io docker-compose docker-compose-v2 docker-doc docker-buildx podman-docker containerd runc; do
    if dpkg-query -W -f='${db:Status-Abbrev}' "$package" 2>/dev/null | grep -q '^ii'; then
      conflicting_packages+=("$package")
    fi
  done
  if ((${#conflicting_packages[@]})); then
    fail "Docker's official packages conflict with installed packages: ${conflicting_packages[*]}"
  fi
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL "https://download.docker.com/linux/$ID/gpg" -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
  codename=${UBUNTU_CODENAME:-${VERSION_CODENAME:?distribution codename is unavailable}}
  cat > /etc/apt/sources.list.d/docker.sources <<EOF
Types: deb
URIs: https://download.docker.com/linux/$ID
Suites: $codename
Components: stable
Architectures: $arch
Signed-By: /etc/apt/keyrings/docker.asc
EOF
  apt-get update
  apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
command -v systemctl >/dev/null 2>&1 || fail 'systemd is required to manage Docker on this server'
systemctl enable --now docker
docker info >/dev/null 2>&1 || fail 'Docker daemon is not available after installation'
compose_version=$(docker compose version --short 2>/dev/null | sed 's/^v//') \
  || fail 'could not read the Docker Compose version'
[[ $compose_version =~ ^[0-9]+([.][0-9]+){1,2}$ ]] \
  || fail "Docker Compose returned an unsupported version string: $compose_version"
dpkg --compare-versions "$compose_version" ge 2.17 \
  || fail "Docker Compose 2.17 or newer is required (found $compose_version)"

install_parent=$(dirname -- "$install_dir")
install -d -m 0755 "$install_parent"
staging_dir=$(mktemp -d "$install_parent/.ket-install.XXXXXX")
cleanup_staging() {
  local status=$?
  trap - EXIT
  if [[ -n ${staging_dir:-} && -d $staging_dir ]]; then
    rm -rf -- "$staging_dir"
  fi
  exit "$status"
}
trap cleanup_staging EXIT
git clone --depth 1 --branch "$git_ref" "$REPOSITORY_URL" "$staging_dir"
cd "$staging_dir"

umask 077
xray_keys=$(docker run --rm "$XRAY_IMAGE" x25519)
xray_private=$(sed -n 's/^PrivateKey: //p' <<<"$xray_keys")
xray_public=$(sed -n 's/^Password (PublicKey): //p' <<<"$xray_keys")
[[ $xray_private =~ ^[A-Za-z0-9_-]{43}$ && $xray_public =~ ^[A-Za-z0-9_-]{43}$ ]] \
  || fail 'Xray did not return a valid X25519 key pair'
wireguard_private=$(wg genkey)
wireguard_public=$(wg pubkey <<<"$wireguard_private")

node_id=$(printf '%s-%s' "${country_code,,}" "$direct_host" \
  | tr -c 'a-z0-9._-' '-' | cut -c1-64)

cat > .env <<EOF
KET_ADMIN_TOKEN=$(random_secret)
KET_PUBLIC_URL=https://$domain
KET_CONTROL_PORT=8787
KET_CERTBOT_NAME=$domain
KET_NODE_ID=$node_id
KET_NODE_NAME="Ket $city"
KET_COUNTRY_CODE=$country_code
KET_COUNTRY_NAME="$country_name"
KET_CITY="$city"
KET_LATITUDE=$latitude
KET_LONGITUDE=$longitude
KET_MAX_SESSIONS=$max_sessions
KET_SESSION_TTL_SECONDS=1800
KET_TRANSPORTS_JSON=[]

KET_HYSTERIA_ENABLED=true
KET_HYSTERIA_PUBLIC_HOST=$direct_host
KET_HYSTERIA_PUBLIC_PORT=443
KET_HYSTERIA_SNI=$direct_host
KET_HYSTERIA_STATS_SECRET=$(random_secret)
KET_HYSTERIA_MASQUERADE_URL=https://www.cloudflare.com/
KET_HYSTERIA_TLS_DIR=./secrets/tls
KET_HYSTERIA_TLS_CERT_PATH=/etc/hysteria/tls/fullchain.pem
KET_HYSTERIA_TLS_KEY_PATH=/etc/hysteria/tls/privkey.pem
KET_HYSTERIA_OBFS=salamander
KET_HYSTERIA_OBFS_PASSWORD=$(random_secret)

KET_XRAY_ENABLED=true
KET_XRAY_PUBLIC_HOST=$direct_host
KET_XRAY_PUBLIC_PORT=443
KET_XRAY_SNI=www.cloudflare.com
KET_XRAY_SERVER_NAMES=www.cloudflare.com
KET_XRAY_REALITY_TARGET=www.cloudflare.com:443
KET_XRAY_PRIVATE_KEY=$xray_private
KET_XRAY_PUBLIC_KEY=$xray_public
KET_XRAY_SHORT_ID=$(random_hex 8)
KET_XRAY_FINGERPRINT=chrome
KET_XRAY_CREDENTIAL_KEY=$(random_secret)

KET_XHTTP_ENABLED=true
KET_XHTTP_PUBLIC_HOST=$domain
KET_XHTTP_PUBLIC_PORT=443
KET_XHTTP_SNI=$domain
KET_XHTTP_PATH=/ket-xhttp-$(random_hex 16)
KET_XHTTP_FINGERPRINT=chrome
KET_XHTTP_ORIGIN_BIND_ADDRESS=127.0.0.1
KET_XHTTP_ORIGIN_PORT=8445

KET_SHADOWSOCKS_ENABLED=true
KET_SHADOWSOCKS_PUBLIC_HOST=$direct_host
KET_SHADOWSOCKS_PORT_START=$shadowsocks_start
KET_SHADOWSOCKS_PORT_END=$shadowsocks_end
KET_SHADOWSOCKS_CREDENTIAL_KEY=$(random_secret)

KET_WIREGUARD_ENABLED=true
KET_WIREGUARD_PUBLIC_HOST=$domain
KET_WIREGUARD_PUBLIC_PORT=443
KET_WIREGUARD_SNI=$domain
KET_WIREGUARD_WS_PATH_PREFIX=ket-wg-$(random_hex 16)
KET_WIREGUARD_ORIGIN_BIND_ADDRESS=127.0.0.1
KET_WIREGUARD_ORIGIN_PORT=8446
KET_WIREGUARD_SERVER_PRIVATE_KEY=$wireguard_private
KET_WIREGUARD_SERVER_PUBLIC_KEY=$wireguard_public
KET_WIREGUARD_MANAGER_TOKEN=$(random_secret)
KET_WIREGUARD_CREDENTIAL_KEY=$(random_secret)

KET_OPENVPN_ENABLED=true
KET_OPENVPN_PUBLIC_HOST=$direct_host
KET_OPENVPN_PUBLIC_PORT=9443
KET_OPENVPN_SNI=$direct_host
KET_OPENVPN_BIND_ADDRESS=0.0.0.0
KET_OPENVPN_PKI_DIR=./secrets/openvpn
KET_OPENVPN_STUNNEL_TLS_DIR=./secrets/openvpn-stunnel
KET_OPENVPN_MANAGER_TOKEN=$(random_secret)
KET_OPENVPN_AUTH_TOKEN=$(random_secret)

KET_EDGE_BIND_ADDRESS=0.0.0.0
KET_EDGE_PUBLIC_PORT=443
EOF
chmod 0600 .env

install -d -m 0700 secrets/openvpn secrets/openvpn-stunnel
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out secrets/openvpn/ca.key
openssl req -x509 -new -sha256 -days 3650 \
  -key secrets/openvpn/ca.key -subj '/CN=Ket OpenVPN CA' -out secrets/openvpn/ca.crt
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out secrets/openvpn/server.key
openssl req -new -sha256 -key secrets/openvpn/server.key \
  -subj "/CN=$direct_host" -out secrets/openvpn/server.csr
printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyAgreement\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:%s\n' \
  "$direct_host" > secrets/openvpn/server.ext
openssl x509 -req -sha256 -days 825 \
  -in secrets/openvpn/server.csr \
  -CA secrets/openvpn/ca.crt -CAkey secrets/openvpn/ca.key -CAcreateserial \
  -extfile secrets/openvpn/server.ext -out secrets/openvpn/server.crt
rm -f secrets/openvpn/server.csr secrets/openvpn/server.ext secrets/openvpn/ca.srl
chmod 0600 secrets/openvpn/ca.key secrets/openvpn/server.key
chmod 0644 secrets/openvpn/ca.crt secrets/openvpn/server.crt

./packaging/server/refresh-certificates.sh
docker build --tag ket-control-plane:local .
docker run --rm --user 0:0 \
  --entrypoint /usr/local/bin/openvpn \
  -v "$staging_dir/secrets/openvpn:/out" \
  ket-control-plane:local --genkey tls-crypt /out/tls-crypt.key
chmod 0644 secrets/openvpn/tls-crypt.key

set -a
# shellcheck source=/dev/null
. ./.env
set +a
./packaging/validate-env.sh
./packaging/server/compose.sh config --quiet

cd "$install_parent"
mv -- "$staging_dir" "$install_dir"
staging_dir=
trap - EXIT
cd "$install_dir"
./packaging/server/compose.sh up --detach --no-build --remove-orphans

install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
cat > /etc/letsencrypt/renewal-hooks/deploy/ket-server <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd "$install_dir"
exec ./packaging/server/refresh-certificates.sh --restart
EOF
chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/ket-server
if systemctl list-unit-files --type=timer --no-legend certbot.timer 2>/dev/null \
  | grep -q '^certbot[.]timer'; then
  systemctl enable --now certbot.timer
else
  printf 'Certbot timer unit not found; verify this distribution schedules certbot renew through cron.\n'
fi

ready=false
for _ in $(seq 1 60); do
  if curl --fail --silent --show-error \
    --resolve "$domain:443:127.0.0.1" "https://$domain/readyz" >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 2
done
$ready || { ./packaging/server/compose.sh ps; fail 'Ket did not become ready within 120 seconds'; }

grant_file=$(mktemp /run/ket-first-grant.XXXXXX)
trap 'rm -f "$grant_file"' EXIT
jq -n \
  --argjson max_connections "$first_code_max_connections" \
  --argjson valid_for_minutes "$first_code_valid_minutes" \
  '{label:"First devices",max_connections:$max_connections,valid_for_minutes:$valid_for_minutes}' \
  | curl --fail --silent --show-error \
      --request POST \
      --header "Authorization: Bearer $KET_ADMIN_TOKEN" \
      --header 'Content-Type: application/json' \
      --data-binary @- \
      http://127.0.0.1:8787/v1/admin/access-grants \
  > "$grant_file"
first_code=$(jq -er '.access_code' "$grant_file")
first_code_expires_at=$(jq -er '.expires_at_epoch_seconds' "$grant_file")
rm -f "$grant_file"
trap - EXIT

printf '\nKet is ready at https://%s\n' "$domain"
printf 'First 32-character access code: %s\n' "$first_code"
printf 'First access code concurrent-connection limit: %s\n' "$first_code_max_connections"
printf 'First access code expires at Unix time: %s (%s minutes after creation)\n' \
  "$first_code_expires_at" "$first_code_valid_minutes"
printf 'Store that code now; the server never stores its plaintext value.\n'
printf '\nClient setup:\n'
printf '  1. Download Linux, Windows, or Android client: %s\n' "$CLIENT_RELEASE_URL"
printf '  2. Server URL: https://%s\n' "$domain"
printf '  3. Enter the access code printed above, then connect.\n'
printf '\nRequired cloud ingress (verify it remains open):\n'
print_ingress
printf 'Manage the stack with: cd %s && ./packaging/server/compose.sh ps\n' "$install_dir"
