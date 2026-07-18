#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'Ket configuration error: %s\n' "$1" >&2; exit 1; }
required() { [[ -n "${!1:-}" ]] || fail "$1 is required"; }
valid_port() { [[ "${!1:-$2}" =~ ^[1-9][0-9]*$ ]] && (( ${!1:-$2} <= 65535 )); }

required KET_ADMIN_TOKEN
(( ${#KET_ADMIN_TOKEN} >= 32 )) || fail 'KET_ADMIN_TOKEN must contain at least 32 characters'
[[ "${KET_SESSION_TTL_SECONDS:-1800}" =~ ^[0-9]+$ ]] || fail 'KET_SESSION_TTL_SECONDS must be numeric'
(( KET_SESSION_TTL_SECONDS >= 60 && KET_SESSION_TTL_SECONDS <= 86400 )) || fail 'KET_SESSION_TTL_SECONDS must be between 60 and 86400'
[[ "${KET_MAX_SESSIONS:-1000}" =~ ^[1-9][0-9]*$ ]] || fail 'KET_MAX_SESSIONS must be a positive integer'
[[ "${KET_COUNTRY_CODE:-ZZ}" =~ ^[A-Z]{2}$ ]] || fail 'KET_COUNTRY_CODE must be two uppercase letters'

if [[ "${KET_HYSTERIA_ENABLED:-false}" == true ]]; then
  required KET_HYSTERIA_PUBLIC_HOST
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
  [[ "$KET_XRAY_PUBLIC_HOST" != *://* && "$KET_XRAY_PUBLIC_HOST" != */* ]] || fail 'KET_XRAY_PUBLIC_HOST must be a hostname or IP address without a scheme or path'
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
