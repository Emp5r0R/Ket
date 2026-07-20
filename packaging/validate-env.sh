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
  required KET_XRAY_CREDENTIAL_KEY
  (( ${#KET_XRAY_CREDENTIAL_KEY} >= 32 )) || fail 'KET_XRAY_CREDENTIAL_KEY must contain at least 32 characters'
  case "${KET_XRAY_FINGERPRINT:-chrome}" in
    chrome|firefox|safari|ios|android|edge|random) ;;
    *) fail 'KET_XRAY_FINGERPRINT is unsupported' ;;
  esac
fi

printf 'Ket configuration preflight passed.\n'
