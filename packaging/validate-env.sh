#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'Ket configuration error: %s\n' "$1" >&2; exit 1; }
required() { [[ -n "${!1:-}" ]] || fail "$1 is required"; }
valid_port() { [[ "${!1:-$2}" =~ ^[1-9][0-9]*$ ]] && (( ${!1:-$2} <= 65535 )); }
bounded_text() {
  local name=$1 value=${!1:-$2} maximum=$3
  [[ -n "$value" && ${#value} -le $maximum ]] || return 1
  [[ "$value" != [[:space:]]* && "$value" != *[[:space:]] ]] || return 1
  [[ ! "$value" =~ [[:cntrl:]] ]]
}
valid_host() {
  local value=${!1:-}
  [[ -n "$value" && ${#value} -le 253 ]] || return 1
  [[ "$value" != *://* && "$value" != */* && "$value" != *\\* ]] || return 1
  [[ "$value" != *'?'* && "$value" != *'#'* ]] || return 1
  [[ ! "$value" =~ [[:space:][:cntrl:]] ]]
}
valid_wireguard_key() {
  local value=${!1:-}
  [[ "$value" =~ ^[A-Za-z0-9+/]{43}=$ ]]
}
valid_material_file() {
  local path=$1 begin=$2 end=$3 size
  [[ -f "$path" && -r "$path" ]] || return 1
  size=$(wc -c < "$path")
  (( size > 0 && size <= 3072 )) || return 1
  grep -Fqx -- "$begin" "$path" && grep -Fqx -- "$end" "$path"
}
valid_public_url() {
  local value=${KET_PUBLIC_URL:-http://127.0.0.1:8787} authority
  [[ -n "$value" && ${#value} -le 2048 ]] || return 1
  [[ "$value" == http://* || "$value" == https://* ]] || return 1
  [[ "$value" != *'?'* && "$value" != *'#'* ]] || return 1
  [[ ! "$value" =~ [[:space:][:cntrl:]] ]] || return 1
  authority=${value#*://}
  authority=${authority%%/*}
  [[ -n "$authority" && "$authority" != *'@'* ]] || return 1
  if [[ "$value" == http://* ]]; then
    case "$authority" in
      localhost|localhost:*|127.0.0.1|127.0.0.1:*|'[::1]'|'[::1]':*) ;;
      *) return 1 ;;
    esac
  fi
}

required KET_ADMIN_TOKEN
(( ${#KET_ADMIN_TOKEN} >= 32 )) || fail 'KET_ADMIN_TOKEN must contain at least 32 characters'
valid_public_url || fail 'KET_PUBLIC_URL must use HTTPS or loopback HTTP without credentials, whitespace, a query, or a fragment'
[[ "${KET_NODE_ID:-ket-node-1}" =~ ^[A-Za-z0-9._-]{1,128}$ ]] || fail 'KET_NODE_ID must be a 1-128 character ASCII identifier'
bounded_text KET_NODE_NAME 'Ket node' 128 || fail 'KET_NODE_NAME must contain 1-128 trimmed printable characters'
bounded_text KET_COUNTRY_NAME 'Unknown' 128 || fail 'KET_COUNTRY_NAME must contain 1-128 trimmed printable characters'
if [[ -n "${KET_CITY:-}" ]]; then
  bounded_text KET_CITY '' 128 || fail 'KET_CITY must contain 1-128 trimmed printable characters'
fi
session_ttl=${KET_SESSION_TTL_SECONDS:-1800}
[[ "$session_ttl" =~ ^[0-9]+$ ]] || fail 'KET_SESSION_TTL_SECONDS must be numeric'
(( session_ttl >= 60 && session_ttl <= 86400 )) || fail 'KET_SESSION_TTL_SECONDS must be between 60 and 86400'
[[ "${KET_MAX_SESSIONS:-1000}" =~ ^[1-9][0-9]*$ ]] || fail 'KET_MAX_SESSIONS must be a positive integer'
[[ "${KET_COUNTRY_CODE:-ZZ}" =~ ^[A-Z]{2}$ ]] || fail 'KET_COUNTRY_CODE must be two uppercase letters'

if [[ "${KET_HYSTERIA_ENABLED:-false}" == true ]]; then
  required KET_HYSTERIA_PUBLIC_HOST
  valid_host KET_HYSTERIA_PUBLIC_HOST || fail 'KET_HYSTERIA_PUBLIC_HOST must be a bounded hostname or IP address'
  required KET_HYSTERIA_STATS_SECRET
  (( ${#KET_HYSTERIA_STATS_SECRET} >= 32 )) || fail 'KET_HYSTERIA_STATS_SECRET must contain at least 32 characters'
  required KET_HYSTERIA_MASQUERADE_URL
  [[ "$KET_HYSTERIA_MASQUERADE_URL" == https://* ]] || fail 'KET_HYSTERIA_MASQUERADE_URL must use HTTPS'
  cert_dir=${KET_HYSTERIA_TLS_DIR:-./secrets/tls}
  [[ -r "$cert_dir/fullchain.pem" ]] || fail "missing readable $cert_dir/fullchain.pem"
  [[ -r "$cert_dir/privkey.pem" ]] || fail "missing readable $cert_dir/privkey.pem"
  if [[ "${KET_HYSTERIA_OBFS:-none}" != none ]]; then
    required KET_HYSTERIA_OBFS_PASSWORD
    (( ${#KET_HYSTERIA_OBFS_PASSWORD} >= 32 )) || fail 'KET_HYSTERIA_OBFS_PASSWORD must contain at least 32 characters'
  fi
fi

if [[ "${KET_SHADOWSOCKS_ENABLED:-false}" == true ]]; then
  required KET_SHADOWSOCKS_PUBLIC_HOST
  valid_host KET_SHADOWSOCKS_PUBLIC_HOST || fail 'KET_SHADOWSOCKS_PUBLIC_HOST must be a bounded hostname or IP address'
  valid_port KET_SHADOWSOCKS_PORT_START 20000 || fail 'KET_SHADOWSOCKS_PORT_START must be between 1 and 65535'
  valid_port KET_SHADOWSOCKS_PORT_END 20999 || fail 'KET_SHADOWSOCKS_PORT_END must be between 1 and 65535'
  shadowsocks_start=${KET_SHADOWSOCKS_PORT_START:-20000}
  shadowsocks_end=${KET_SHADOWSOCKS_PORT_END:-20999}
  (( shadowsocks_end >= shadowsocks_start )) || fail 'KET_SHADOWSOCKS_PORT_END must not be lower than KET_SHADOWSOCKS_PORT_START'
  (( ${KET_MAX_SESSIONS:-1000} <= 1500 )) || fail 'KET_MAX_SESSIONS cannot exceed 1500 when Shadowsocks is enabled'
  (( shadowsocks_end - shadowsocks_start + 1 >= ${KET_MAX_SESSIONS:-1000} )) || fail 'the Shadowsocks port range must contain at least KET_MAX_SESSIONS ports'
  required KET_SHADOWSOCKS_CREDENTIAL_KEY
  (( ${#KET_SHADOWSOCKS_CREDENTIAL_KEY} >= 32 )) || fail 'KET_SHADOWSOCKS_CREDENTIAL_KEY must contain at least 32 characters'
fi

if [[ "${KET_XRAY_ENABLED:-false}" == true ]]; then
  required KET_XRAY_PUBLIC_HOST
  valid_host KET_XRAY_PUBLIC_HOST || fail 'KET_XRAY_PUBLIC_HOST must be a bounded hostname or IP address'
  valid_port KET_XRAY_PUBLIC_PORT 443 || fail 'KET_XRAY_PUBLIC_PORT must be between 1 and 65535'
  required KET_XRAY_SNI
  required KET_XRAY_SERVER_NAMES
  case ",${KET_XRAY_SERVER_NAMES// /}," in
    *",$KET_XRAY_SNI,"*) ;;
    *) fail 'KET_XRAY_SNI must be listed in KET_XRAY_SERVER_NAMES' ;;
  esac
  required KET_XRAY_REALITY_TARGET
  [[ "$KET_XRAY_REALITY_TARGET" =~ ^[^:/[:space:]]+:[1-9][0-9]{0,4}$ ]] || fail 'KET_XRAY_REALITY_TARGET must use host:port format'
  required KET_XRAY_PRIVATE_KEY
  required KET_XRAY_PUBLIC_KEY
  [[ "$KET_XRAY_PRIVATE_KEY" =~ ^[A-Za-z0-9_-]{43}$ ]] || fail 'KET_XRAY_PRIVATE_KEY must be a 43-character base64url X25519 key'
  [[ "$KET_XRAY_PUBLIC_KEY" =~ ^[A-Za-z0-9_-]{43}$ ]] || fail 'KET_XRAY_PUBLIC_KEY must be a 43-character base64url X25519 key'
  [[ "$KET_XRAY_PRIVATE_KEY" != "$KET_XRAY_PUBLIC_KEY" ]] || fail 'KET_XRAY_PRIVATE_KEY and KET_XRAY_PUBLIC_KEY must differ'
  required KET_XRAY_SHORT_ID
  [[ "$KET_XRAY_SHORT_ID" =~ ^[A-Fa-f0-9]{16}$ ]] || fail 'KET_XRAY_SHORT_ID must contain exactly 16 hexadecimal characters'
  case "${KET_XRAY_FINGERPRINT:-chrome}" in
    chrome|firefox|safari|ios|android|edge|random) ;;
    *) fail 'KET_XRAY_FINGERPRINT is unsupported' ;;
  esac
fi

if [[ "${KET_XHTTP_ENABLED:-false}" == true ]]; then
  required KET_XHTTP_PUBLIC_HOST
  valid_host KET_XHTTP_PUBLIC_HOST || fail 'KET_XHTTP_PUBLIC_HOST must be a bounded hostname or IP address'
  valid_port KET_XHTTP_PUBLIC_PORT 443 || fail 'KET_XHTTP_PUBLIC_PORT must be between 1 and 65535'
  required KET_XHTTP_SNI
  valid_host KET_XHTTP_SNI || fail 'KET_XHTTP_SNI must be a bounded hostname'
  [[ "$KET_XHTTP_SNI" =~ [A-Za-z] ]] || fail 'KET_XHTTP_SNI must be a hostname, not an IP address'
  required KET_XHTTP_PATH
  [[ "$KET_XHTTP_PATH" =~ ^/[A-Za-z0-9/_-]{15,127}$ ]] || fail 'KET_XHTTP_PATH must be a 16-128 character absolute path using letters, numbers, /, -, or _'
  [[ "$KET_XHTTP_PATH" != */ && "$KET_XHTTP_PATH" != *//* ]] || fail 'KET_XHTTP_PATH cannot end in / or contain //'
  valid_port KET_XHTTP_ORIGIN_PORT 8445 || fail 'KET_XHTTP_ORIGIN_PORT must be between 1 and 65535'
  case "${KET_XHTTP_ORIGIN_BIND_ADDRESS:-127.0.0.1}" in
    127.0.0.1|'[::1]') ;;
    *) fail 'KET_XHTTP_ORIGIN_BIND_ADDRESS must remain loopback-only' ;;
  esac
  case "${KET_XHTTP_FINGERPRINT:-chrome}" in
    chrome|firefox|safari|ios|android|edge|random) ;;
    *) fail 'KET_XHTTP_FINGERPRINT is unsupported' ;;
  esac
fi

if [[ "${KET_XRAY_ENABLED:-false}" == true || "${KET_XHTTP_ENABLED:-false}" == true ]]; then
  required KET_XRAY_CREDENTIAL_KEY
  (( ${#KET_XRAY_CREDENTIAL_KEY} >= 32 )) || fail 'KET_XRAY_CREDENTIAL_KEY must contain at least 32 characters'
fi

if [[ "${KET_WIREGUARD_ENABLED:-false}" == true ]]; then
  required KET_WIREGUARD_PUBLIC_HOST
  valid_host KET_WIREGUARD_PUBLIC_HOST || fail 'KET_WIREGUARD_PUBLIC_HOST must be a bounded hostname or IP address'
  valid_port KET_WIREGUARD_PUBLIC_PORT 443 || fail 'KET_WIREGUARD_PUBLIC_PORT must be between 1 and 65535'
  required KET_WIREGUARD_SNI
  valid_host KET_WIREGUARD_SNI || fail 'KET_WIREGUARD_SNI must be a bounded hostname'
  [[ "$KET_WIREGUARD_SNI" =~ [A-Za-z] ]] || fail 'KET_WIREGUARD_SNI must be a hostname, not an IP address'
  required KET_WIREGUARD_WS_PATH_PREFIX
  [[ "$KET_WIREGUARD_WS_PATH_PREFIX" =~ ^[A-Za-z0-9_-]{16,96}$ ]] || fail 'KET_WIREGUARD_WS_PATH_PREFIX must contain 16-96 letters, numbers, - or _ without slashes'
  valid_port KET_WIREGUARD_ORIGIN_PORT 8446 || fail 'KET_WIREGUARD_ORIGIN_PORT must be between 1 and 65535'
  case "${KET_WIREGUARD_ORIGIN_BIND_ADDRESS:-127.0.0.1}" in
    127.0.0.1|'[::1]') ;;
    *) fail 'KET_WIREGUARD_ORIGIN_BIND_ADDRESS must remain loopback-only' ;;
  esac
  required KET_WIREGUARD_SERVER_PRIVATE_KEY
  valid_wireguard_key KET_WIREGUARD_SERVER_PRIVATE_KEY || fail 'KET_WIREGUARD_SERVER_PRIVATE_KEY must be a 32-byte standard-base64 WireGuard key'
  required KET_WIREGUARD_SERVER_PUBLIC_KEY
  valid_wireguard_key KET_WIREGUARD_SERVER_PUBLIC_KEY || fail 'KET_WIREGUARD_SERVER_PUBLIC_KEY must be a 32-byte standard-base64 WireGuard key'
  [[ "$KET_WIREGUARD_SERVER_PRIVATE_KEY" != "$KET_WIREGUARD_SERVER_PUBLIC_KEY" ]] || fail 'WireGuard private and public keys must differ'
  required KET_WIREGUARD_MANAGER_TOKEN
  (( ${#KET_WIREGUARD_MANAGER_TOKEN} >= 32 )) || fail 'KET_WIREGUARD_MANAGER_TOKEN must contain at least 32 characters'
  required KET_WIREGUARD_CREDENTIAL_KEY
  (( ${#KET_WIREGUARD_CREDENTIAL_KEY} >= 32 )) || fail 'KET_WIREGUARD_CREDENTIAL_KEY must contain at least 32 characters'
  (( ${KET_MAX_SESSIONS:-1000} <= 65533 )) || fail 'KET_MAX_SESSIONS cannot exceed 65533 when WireGuard TLS is enabled'
fi

if [[ "${KET_OPENVPN_ENABLED:-false}" == true ]]; then
  required KET_OPENVPN_PUBLIC_HOST
  valid_host KET_OPENVPN_PUBLIC_HOST || fail 'KET_OPENVPN_PUBLIC_HOST must be a bounded hostname or IP address'
  valid_port KET_OPENVPN_PUBLIC_PORT 443 || fail 'KET_OPENVPN_PUBLIC_PORT must be between 1 and 65535'
  required KET_OPENVPN_SNI
  valid_host KET_OPENVPN_SNI || fail 'KET_OPENVPN_SNI must be a bounded hostname'
  [[ "$KET_OPENVPN_SNI" =~ [A-Za-z] ]] || fail 'KET_OPENVPN_SNI must be a hostname, not an IP address'
  required KET_OPENVPN_MANAGER_TOKEN
  (( ${#KET_OPENVPN_MANAGER_TOKEN} >= 32 )) || fail 'KET_OPENVPN_MANAGER_TOKEN must contain at least 32 characters'
  required KET_OPENVPN_AUTH_TOKEN
  (( ${#KET_OPENVPN_AUTH_TOKEN} >= 32 )) || fail 'KET_OPENVPN_AUTH_TOKEN must contain at least 32 characters'
  [[ "$KET_OPENVPN_MANAGER_TOKEN" != "$KET_OPENVPN_AUTH_TOKEN" ]] || fail 'KET_OPENVPN_MANAGER_TOKEN and KET_OPENVPN_AUTH_TOKEN must be independent'
  (( ${KET_MAX_SESSIONS:-1000} <= 65533 )) || fail 'KET_MAX_SESSIONS cannot exceed 65533 when OpenVPN is enabled'

  openvpn_pki_dir=${KET_OPENVPN_PKI_DIR:-./secrets/openvpn}
  openvpn_stunnel_tls_dir=${KET_OPENVPN_STUNNEL_TLS_DIR:-./secrets/openvpn-stunnel}
  for path in \
    "$openvpn_pki_dir/ca.crt" \
    "$openvpn_pki_dir/server.crt" \
    "$openvpn_pki_dir/server.key" \
    "$openvpn_stunnel_tls_dir/fullchain.pem" \
    "$openvpn_stunnel_tls_dir/privkey.pem"; do
    [[ -f "$path" && -r "$path" ]] || fail "missing readable $path"
  done
  valid_material_file "$openvpn_pki_dir/ca.crt" '-----BEGIN CERTIFICATE-----' '-----END CERTIFICATE-----' || fail 'OpenVPN ca.crt must be a non-empty PEM certificate no larger than 3 KiB'
  valid_material_file "$openvpn_pki_dir/stunnel-ca.crt" '-----BEGIN CERTIFICATE-----' '-----END CERTIFICATE-----' || fail 'OpenVPN stunnel-ca.crt must be a non-empty PEM certificate no larger than 3 KiB'
  valid_material_file "$openvpn_pki_dir/tls-crypt.key" '-----BEGIN OpenVPN Static key V1-----' '-----END OpenVPN Static key V1-----' || fail 'OpenVPN tls-crypt.key must be a non-empty V1 key no larger than 3 KiB'

  if [[ "${KET_XRAY_ENABLED:-false}" == true \
    && "${KET_OPENVPN_PUBLIC_PORT:-443}" == "${KET_XRAY_PUBLIC_PORT:-443}" \
    && "${KET_OPENVPN_BIND_ADDRESS:-0.0.0.0}" == "${KET_XRAY_BIND_ADDRESS:-0.0.0.0}" ]]; then
    fail 'OpenVPN/stunnel and VLESS + REALITY cannot bind the same TCP address and port'
  fi
fi

printf 'Ket configuration preflight passed.\n'
