#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VALIDATOR="$ROOT/packaging/validate-env.sh"
TEMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TEMP_ROOT"' EXIT

BASE_ENV=(
  "PATH=$PATH"
  "HOME=${HOME:-/tmp}"
  KET_ADMIN_TOKEN=01234567890123456789012345678901
  KET_PUBLIC_URL=https://ket.example.test
  KET_SESSION_TTL_SECONDS=1800
  KET_MAX_SESSIONS=5
  KET_COUNTRY_CODE=SG
  KET_COUNTRY_NAME=Singapore
)

passed=0
expect_pass() {
  local label=$1
  shift
  local output
  if ! output=$(env -i "${BASE_ENV[@]}" "$@" "$VALIDATOR" 2>&1); then
    printf 'FAIL: %s should pass\n%s\n' "$label" "$output" >&2
    exit 1
  fi
  [[ "$output" == *'Ket configuration preflight passed.'* ]] || {
    printf 'FAIL: %s did not report success\n%s\n' "$label" "$output" >&2
    exit 1
  }
  ((passed += 1))
}

expect_fail() {
  local label=$1 expected=$2
  shift 2
  local output status=0
  output=$(env -i "${BASE_ENV[@]}" "$@" "$VALIDATOR" 2>&1) || status=$?
  if (( status == 0 )); then
    printf 'FAIL: %s should fail\n' "$label" >&2
    exit 1
  fi
  [[ "$output" == *"$expected"* ]] || {
    printf 'FAIL: %s returned the wrong error\nexpected: %s\nactual: %s\n' \
      "$label" "$expected" "$output" >&2
    exit 1
  }
  ((passed += 1))
}

tls_dir="$TEMP_ROOT/hysteria-tls"
pki_dir="$TEMP_ROOT/openvpn"
stunnel_dir="$TEMP_ROOT/openvpn-stunnel"
mkdir -p "$tls_dir" "$pki_dir" "$stunnel_dir"
printf '%s\n' CERT > "$tls_dir/fullchain.pem"
printf '%s\n' KEY > "$tls_dir/privkey.pem"
printf '%s\n' '-----BEGIN CERTIFICATE-----' TEST '-----END CERTIFICATE-----' > "$pki_dir/ca.crt"
cp "$pki_dir/ca.crt" "$pki_dir/stunnel-ca.crt"
cp "$pki_dir/ca.crt" "$pki_dir/server.crt"
printf '%s\n' '-----BEGIN PRIVATE KEY-----' TEST '-----END PRIVATE KEY-----' > "$pki_dir/server.key"
printf '%s\n' '-----BEGIN OpenVPN Static key V1-----' TEST '-----END OpenVPN Static key V1-----' > "$pki_dir/tls-crypt.key"
cp "$pki_dir/ca.crt" "$stunnel_dir/fullchain.pem"
cp "$pki_dir/server.key" "$stunnel_dir/privkey.pem"

HYSTERIA_ENV=(
  KET_HYSTERIA_ENABLED=true
  KET_HYSTERIA_PUBLIC_HOST=vpn.example.test
  KET_HYSTERIA_STATS_SECRET=stats-secret-with-at-least-32-characters
  KET_HYSTERIA_MASQUERADE_URL=https://www.example.com/
  "KET_HYSTERIA_TLS_DIR=$tls_dir"
  KET_HYSTERIA_OBFS=gecko
  KET_HYSTERIA_OBFS_PASSWORD=obfuscation-password-at-least-32-characters
)
SHADOWSOCKS_ENV=(
  KET_SHADOWSOCKS_ENABLED=true
  KET_SHADOWSOCKS_PUBLIC_HOST=vpn.example.test
  KET_SHADOWSOCKS_PORT_START=20000
  KET_SHADOWSOCKS_PORT_END=20004
  KET_SHADOWSOCKS_CREDENTIAL_KEY=credential-key-with-at-least-32-characters
)
XRAY_ENV=(
  KET_XRAY_ENABLED=true
  KET_XRAY_PUBLIC_HOST=vpn.example.test
  KET_XRAY_PUBLIC_PORT=443
  KET_XRAY_SNI=www.cloudflare.com
  KET_XRAY_SERVER_NAMES=www.cloudflare.com,www.example.com
  KET_XRAY_REALITY_TARGET=www.cloudflare.com:443
  KET_XRAY_PRIVATE_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
  KET_XRAY_PUBLIC_KEY=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
  KET_XRAY_SHORT_ID=0123456789abcdef
  KET_XRAY_CREDENTIAL_KEY=credential-key-with-at-least-32-characters
)
XHTTP_ENV=(
  KET_XHTTP_ENABLED=true
  KET_XHTTP_PUBLIC_HOST=stealth.example.test
  KET_XHTTP_PUBLIC_PORT=443
  KET_XHTTP_SNI=stealth.example.test
  KET_XHTTP_PATH=/ket-a1b2c3d4e5f6a1b2c3d4e5f6
  KET_XHTTP_ORIGIN_BIND_ADDRESS=127.0.0.1
  KET_XHTTP_ORIGIN_PORT=8445
  KET_XRAY_CREDENTIAL_KEY=credential-key-with-at-least-32-characters
)
WIREGUARD_ENV=(
  KET_WIREGUARD_ENABLED=true
  KET_WIREGUARD_PUBLIC_HOST=wireguard.example.test
  KET_WIREGUARD_PUBLIC_PORT=443
  KET_WIREGUARD_SNI=wireguard.example.test
  KET_WIREGUARD_WS_PATH_PREFIX=ket-wireguard-01
  KET_WIREGUARD_ORIGIN_BIND_ADDRESS=127.0.0.1
  KET_WIREGUARD_ORIGIN_PORT=8446
  KET_WIREGUARD_SERVER_PRIVATE_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
  KET_WIREGUARD_SERVER_PUBLIC_KEY=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=
  KET_WIREGUARD_MANAGER_TOKEN=manager-token-with-at-least-32-characters
  KET_WIREGUARD_CREDENTIAL_KEY=credential-key-with-at-least-32-characters
)
OPENVPN_ENV=(
  KET_OPENVPN_ENABLED=true
  KET_OPENVPN_PUBLIC_HOST=openvpn.example.test
  KET_OPENVPN_PUBLIC_PORT=443
  KET_OPENVPN_SNI=openvpn.example.test
  KET_OPENVPN_MANAGER_TOKEN=manager-token-with-at-least-32-characters
  KET_OPENVPN_AUTH_TOKEN=independent-auth-token-at-least-32-characters
  "KET_OPENVPN_PKI_DIR=$pki_dir"
  "KET_OPENVPN_STUNNEL_TLS_DIR=$stunnel_dir"
)

expect_pass 'base control-plane configuration'
expect_pass 'Hysteria2 configuration' "${HYSTERIA_ENV[@]}"
expect_pass 'Shadowsocks 2022 configuration' "${SHADOWSOCKS_ENV[@]}"
expect_pass 'VLESS REALITY configuration' "${XRAY_ENV[@]}"
expect_pass 'HTTPS Stealth configuration' "${XHTTP_ENV[@]}"
expect_pass 'WireGuard TLS configuration' "${WIREGUARD_ENV[@]}"
expect_pass 'OpenVPN TLS configuration' "${OPENVPN_ENV[@]}"

expect_fail 'missing admin token' 'KET_ADMIN_TOKEN is required' KET_ADMIN_TOKEN=
expect_fail 'remote plaintext control URL' 'KET_PUBLIC_URL must use HTTPS' KET_PUBLIC_URL=http://ket.example.test
expect_fail 'invalid country code' 'KET_COUNTRY_CODE must be two uppercase letters' KET_COUNTRY_CODE=sg
expect_fail 'short session lifetime' 'KET_SESSION_TTL_SECONDS must be between 60 and 86400' KET_SESSION_TTL_SECONDS=59
expect_fail 'Hysteria weak obfuscation secret' 'KET_HYSTERIA_OBFS_PASSWORD must contain at least 32 characters' \
  "${HYSTERIA_ENV[@]}" KET_HYSTERIA_OBFS_PASSWORD=short
expect_fail 'Shadowsocks undersized port pool' 'the Shadowsocks port range must contain at least KET_MAX_SESSIONS ports' \
  "${SHADOWSOCKS_ENV[@]}" KET_SHADOWSOCKS_PORT_END=20003
expect_fail 'REALITY equal key pair' 'KET_XRAY_PRIVATE_KEY and KET_XRAY_PUBLIC_KEY must differ' \
  "${XRAY_ENV[@]}" KET_XRAY_PUBLIC_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
expect_fail 'REALITY SNI outside allowlist' 'KET_XRAY_SNI must be listed in KET_XRAY_SERVER_NAMES' \
  "${XRAY_ENV[@]}" KET_XRAY_SERVER_NAMES=www.example.com
expect_fail 'Stealth non-loopback origin' 'KET_XHTTP_ORIGIN_BIND_ADDRESS must remain loopback-only' \
  "${XHTTP_ENV[@]}" KET_XHTTP_ORIGIN_BIND_ADDRESS=0.0.0.0
expect_fail 'Stealth ambiguous path' 'KET_XHTTP_PATH cannot end in / or contain //' \
  "${XHTTP_ENV[@]}" KET_XHTTP_PATH=/ket-a1b2c3d4e5f6//bad
expect_fail 'WireGuard equal key pair' 'WireGuard private and public keys must differ' \
  "${WIREGUARD_ENV[@]}" KET_WIREGUARD_SERVER_PUBLIC_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
expect_fail 'WireGuard public origin' 'KET_WIREGUARD_ORIGIN_BIND_ADDRESS must remain loopback-only' \
  "${WIREGUARD_ENV[@]}" KET_WIREGUARD_ORIGIN_BIND_ADDRESS=0.0.0.0
expect_fail 'OpenVPN shared manager and auth tokens' 'must be independent' \
  "${OPENVPN_ENV[@]}" KET_OPENVPN_AUTH_TOKEN=manager-token-with-at-least-32-characters
expect_fail 'OpenVPN and REALITY listener collision' 'cannot bind the same TCP address and port' \
  "${XRAY_ENV[@]}" "${OPENVPN_ENV[@]}"

printf 'validate-env contract tests passed (%d cases).\n' "$passed"
