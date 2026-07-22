#!/usr/bin/env bash
set -euo pipefail

readonly REPOSITORY_URL=https://github.com/Emp5r0R/Ket.git
readonly XRAY_IMAGE='ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7'

usage() {
  cat <<'EOF'
Install a complete Ket server on Debian 12+ or Ubuntu 22.04+.

Usage:
  install-server.sh --domain HOST --email EMAIL [options]

Required:
  --domain HOST          HTTPS control/XHTTP/WireGuard hostname
  --email EMAIL          Let's Encrypt account email

Modes:
  --mode direct          Normal VPS; HOST resolves directly to this VPS (default)
  --mode cloudflare      HOST is Cloudflare-proxied; --direct-host is DNS-only
  --direct-host HOST     Raw Hysteria/REALITY/Shadowsocks/OpenVPN hostname

Location and capacity:
  Location is detected from the VPS public IP by default. To override it,
  provide all five location options together:
  --country-code XX      Two-letter country code
  --country-name NAME    Display country
  --city NAME            Display city
  --latitude NUMBER      Map latitude
  --longitude NUMBER     Map longitude
  --max-sessions N       Maximum concurrent sessions, 1-512 (default: 32)

Other:
  --ref REF              Git tag or branch to install (default: main)
  --install-dir PATH     Installation directory (default: /opt/ket)
  --plan                 Validate and print the required ingress without changes
  --help                 Show this help

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
  [[ -n $1 && ${#1} -le 64 && $1 =~ ^[A-Za-z0-9][A-Za-z0-9.,_\ -]*$ ]]
}
validate_location() {
  [[ $country_code =~ ^[A-Z]{2}$ ]] || fail '--country-code must contain two letters'
  valid_display_text "$country_name" || fail '--country-name contains unsupported characters'
  valid_display_text "$city" || fail '--city contains unsupported characters'
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

mode=direct
domain=
direct_host=
email=
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
git_ref=main
install_dir=/opt/ket
plan=false

while (($#)); do
  case "$1" in
    --domain) need_value "$@"; domain=$2; shift 2 ;;
    --direct-host) need_value "$@"; direct_host=$2; shift 2 ;;
    --email) need_value "$@"; email=$2; shift 2 ;;
    --mode) need_value "$@"; mode=$2; shift 2 ;;
    --country-code) need_value "$@"; country_code=${2^^}; country_code_set=true; shift 2 ;;
    --country-name) need_value "$@"; country_name=$2; country_name_set=true; shift 2 ;;
    --city) need_value "$@"; city=$2; city_set=true; shift 2 ;;
    --latitude) need_value "$@"; latitude=$2; latitude_set=true; shift 2 ;;
    --longitude) need_value "$@"; longitude=$2; longitude_set=true; shift 2 ;;
    --max-sessions) need_value "$@"; max_sessions=$2; shift 2 ;;
    --ref) need_value "$@"; git_ref=$2; shift 2 ;;
    --install-dir) need_value "$@"; install_dir=$2; shift 2 ;;
    --plan) plan=true; shift ;;
    --help|-h) usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[[ -n $domain ]] || fail '--domain is required'
[[ -n $email ]] || fail '--email is required'
valid_hostname "$domain" || fail '--domain must be a valid hostname'
[[ $email =~ ^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$ ]] || fail '--email is invalid'
case "$mode" in direct|cloudflare) ;; *) fail '--mode must be direct or cloudflare' ;; esac

if [[ -z $direct_host ]]; then
  [[ $mode == direct ]] || fail '--direct-host is required in cloudflare mode'
  direct_host=$domain
fi
valid_hostname "$direct_host" || fail '--direct-host must be a valid hostname'
if [[ $mode == cloudflare && $direct_host == "$domain" ]]; then
  fail 'cloudflare mode requires a separate DNS-only --direct-host'
fi
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
[[ $max_sessions =~ ^[1-9][0-9]*$ ]] && ((max_sessions <= 512)) \
  || fail '--max-sessions must be between 1 and 512'
[[ $git_ref =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ && $git_ref != *..* ]] \
  || fail '--ref is invalid'
[[ $install_dir == /* && $install_dir != / && $install_dir != *$'\n'* ]] \
  || fail '--install-dir must be a safe absolute path'

shadowsocks_start=20000
shadowsocks_end=$((shadowsocks_start + max_sessions - 1))

if $plan; then
  printf 'Mode: %s\nControl hostname: %s\nRaw transport hostname: %s\n' "$mode" "$domain" "$direct_host"
  printf 'Required ingress: TCP 80,443,8443,9443,%s-%s; UDP 443,%s-%s\n' \
    "$shadowsocks_start" "$shadowsocks_end" "$shadowsocks_start" "$shadowsocks_end"
  if [[ $location_mode == automatic ]]; then
    printf 'Location: automatic public-IP detection\n'
  else
    printf 'Location: %s, %s (%s; %s,%s)\n' "$city" "$country_name" "$country_code" "$latitude" "$longitude"
  fi
  printf 'Install directory: %s\nGit ref: %s\n' "$install_dir" "$git_ref"
  exit 0
fi

[[ $EUID -eq 0 ]] || fail 'run the installer as root (curl ... | sudo bash -s -- ...)'
[[ ! -e $install_dir ]] || fail "$install_dir already exists; keep its .env and use the documented upgrade procedure"

if [[ ! -r /etc/os-release ]]; then
  fail 'cannot detect the Linux distribution'
fi
. /etc/os-release
case "${ID:-}" in debian|ubuntu) ;; *) fail 'only Debian and Ubuntu are currently supported' ;; esac

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y ca-certificates certbot curl git gnupg iproute2 jq openssl wireguard-tools

if [[ $location_mode == automatic ]]; then
  detect_location || fail 'could not detect VPS location; rerun with all five location options'
  validate_location
  printf 'Detected VPS location: %s, %s (%s; %s,%s)\n' \
    "$city" "$country_name" "$country_code" "$latitude" "$longitude"
fi

if ! docker compose version >/dev/null 2>&1; then
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL "https://download.docker.com/linux/$ID/gpg" \
    | gpg --dearmor --yes -o /etc/apt/keyrings/docker.gpg
  chmod a+r /etc/apt/keyrings/docker.gpg
  arch=$(dpkg --print-architecture)
  codename=${VERSION_CODENAME:?distribution codename is unavailable}
  printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/%s %s stable\n' \
    "$arch" "$ID" "$codename" > /etc/apt/sources.list.d/docker.list
  apt-get update
  apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  systemctl enable --now docker
fi

if ss -H -ltn 2>/dev/null | awk '{print $4}' | grep -Eq '(^|:)(80)$'; then
  fail 'TCP port 80 is already in use; it must be free for the initial certificate request and renewal'
fi

install_parent=$(dirname -- "$install_dir")
install -d -m 0755 "$install_parent"
git clone --depth 1 --branch "$git_ref" "$REPOSITORY_URL" "$install_dir"
cd "$install_dir"

certbot_args=(
  certonly --standalone --non-interactive --agree-tos --no-eff-email
  --preferred-challenges http --email "$email" --cert-name "$domain" -d "$domain"
)
if [[ $direct_host != "$domain" ]]; then
  certbot_args+=(-d "$direct_host")
fi
certbot "${certbot_args[@]}"

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
KET_XRAY_PUBLIC_PORT=8443
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
  -v "$install_dir/secrets/openvpn:/out" \
  ket-control-plane:local --genkey tls-crypt /out/tls-crypt.key
chmod 0644 secrets/openvpn/tls-crypt.key

set -a
. ./.env
set +a
./packaging/validate-env.sh
./packaging/server/compose.sh config --quiet
./packaging/server/compose.sh up --detach --no-build --remove-orphans

install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
cat > /etc/letsencrypt/renewal-hooks/deploy/ket-server <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd "$install_dir"
exec ./packaging/server/refresh-certificates.sh --restart
EOF
chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/ket-server

if command -v ufw >/dev/null 2>&1 && ufw status | grep -q '^Status: active'; then
  ufw allow 80/tcp comment 'Ket ACME renewal'
  ufw allow 443/tcp comment 'Ket HTTPS transports'
  ufw allow 443/udp comment 'Ket Hysteria2'
  ufw allow 8443/tcp comment 'Ket VLESS REALITY'
  ufw allow 9443/tcp comment 'Ket OpenVPN stunnel'
  ufw allow "$shadowsocks_start:$shadowsocks_end/tcp" comment 'Ket Shadowsocks TCP'
  ufw allow "$shadowsocks_start:$shadowsocks_end/udp" comment 'Ket Shadowsocks UDP'
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
jq -n '{label:"First devices",max_connections:5,expires_at_epoch_seconds:null}' \
  | curl --fail --silent --show-error \
      --request POST \
      --header "Authorization: Bearer $KET_ADMIN_TOKEN" \
      --header 'Content-Type: application/json' \
      --data-binary @- \
      http://127.0.0.1:8787/v1/admin/access-grants \
  > "$grant_file"
first_code=$(jq -er '.access_code' "$grant_file")
rm -f "$grant_file"
trap - EXIT

printf '\nKet is ready at https://%s\n' "$domain"
printf 'First 32-character access code: %s\n' "$first_code"
printf 'Store that code now; the server never stores its plaintext value.\n'
printf 'Open these cloud firewall ports: TCP 80,443,8443,9443,%s-%s and UDP 443,%s-%s.\n' \
  "$shadowsocks_start" "$shadowsocks_end" "$shadowsocks_start" "$shadowsocks_end"
printf 'Manage the stack with: cd %s && ./packaging/server/compose.sh ps\n' "$install_dir"
