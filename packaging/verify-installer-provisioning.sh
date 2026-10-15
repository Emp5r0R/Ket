#!/usr/bin/env bash
# Runs the installer's real provisioning steps (Docker-generated REALITY and
# WireGuard keys, OpenVPN PKI, .env rendering) in a scratch tree and checks the
# result with the server preflight. Needs Docker and openssl; installs nothing.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for dependency in docker openssl jq; do
  command -v "$dependency" >/dev/null || { printf '%s is required.\n' "$dependency" >&2; exit 1; }
done
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-provisioning.XXXXXX")
trap 'rm -rf "$work"' EXIT

# shellcheck source=/dev/null
KET_INSTALLER_LIBRARY=1 . "$root/packaging/install-server.sh"
set -euo pipefail

fail_check() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

for wireguard_enabled in true false; do
  tree="$work/$wireguard_enabled"
  mkdir -p "$tree"
  (
    cd "$tree"
    # Globals normally set by install-server.sh.
    # shellcheck disable=SC2034
    {
      domain=ket.example.com direct_host=ket-8-8-8-8.sslip.io
      country_code=SG country_name=Singapore city=Singapore latitude=1.29 longitude=103.85
      max_sessions=4 shadowsocks_start=20000 shadowsocks_end=20003
    }
    umask 077
    write_environment_file
    create_openvpn_pki
    # Stand-ins for what certificate issuance and refresh-certificates.sh
    # produce on a real server (those steps are unchanged by this phase).
    install -d secrets/tls
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 1 \
      -subj "/CN=$domain" -keyout secrets/tls/privkey.pem -out secrets/tls/fullchain.pem 2>/dev/null
    cp secrets/tls/fullchain.pem secrets/tls/privkey.pem secrets/openvpn-stunnel/
    cp secrets/tls/fullchain.pem secrets/openvpn/stunnel-ca.crt
    {
      printf -- '-----BEGIN OpenVPN Static key V1-----\n'
      for _ in $(seq 1 16); do openssl rand -hex 16; done
      printf -- '-----END OpenVPN Static key V1-----\n'
    } > secrets/openvpn/tls-crypt.key
    [[ $(stat -c %a .env) == 600 ]] || fail_check '.env is not private'
    env -i "PATH=$PATH" bash -c "set -a; . ./.env; set +a; '$root/packaging/validate-env.sh'" >/dev/null 2>"$work/preflight" \
      || { cat "$work/preflight" >&2; fail_check "preflight rejected the generated .env (wireguard=$wireguard_enabled)"; }
    grep -Fqx "KET_WIREGUARD_ENABLED=$wireguard_enabled" .env || fail_check 'WireGuard flag not rendered'
    grep -Eq '^KET_WIREGUARD_SERVER_PRIVATE_KEY=[A-Za-z0-9+/]{43}=$' .env || fail_check 'WireGuard key is not standard Base64'
    grep -Eq '^KET_XRAY_PRIVATE_KEY=[A-Za-z0-9_-]{43}$' .env || fail_check 'REALITY key is not Base64URL'
    openssl verify -CAfile secrets/openvpn/ca.crt secrets/openvpn/server.crt >/dev/null \
      || fail_check 'OpenVPN server certificate does not chain to its CA'
  )
done

printf 'Installer provisioning verification passed.\n'
