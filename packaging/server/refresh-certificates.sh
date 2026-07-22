#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID} -eq 0 ]] || { printf 'Run as root.\n' >&2; exit 1; }

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"
set -a
. ./.env
set +a

cert_name=${KET_CERTBOT_NAME:?KET_CERTBOT_NAME is required}
source_dir="/etc/letsencrypt/live/$cert_name"
tls_dir=${KET_HYSTERIA_TLS_DIR:-./secrets/tls}
openvpn_dir=${KET_OPENVPN_PKI_DIR:-./secrets/openvpn}
stunnel_dir=${KET_OPENVPN_STUNNEL_TLS_DIR:-./secrets/openvpn-stunnel}

for file in cert.pem fullchain.pem privkey.pem chain.pem; do
  [[ -r "$source_dir/$file" ]] || { printf 'Missing certificate file: %s\n' "$source_dir/$file" >&2; exit 1; }
done

install -d -m 0750 -o root -g 10001 "$tls_dir"
install -d -m 0700 -o root -g root "$openvpn_dir" "$stunnel_dir"

install -m 0640 -o root -g 10001 "$source_dir/fullchain.pem" "$tls_dir/fullchain.pem.next"
install -m 0640 -o root -g 10001 "$source_dir/privkey.pem" "$tls_dir/privkey.pem.next"
mv -f "$tls_dir/fullchain.pem.next" "$tls_dir/fullchain.pem"
mv -f "$tls_dir/privkey.pem.next" "$tls_dir/privkey.pem"

install -m 0644 -o root -g root "$source_dir/fullchain.pem" "$stunnel_dir/fullchain.pem.next"
install -m 0600 -o root -g root "$source_dir/privkey.pem" "$stunnel_dir/privkey.pem.next"
"$root/packaging/server/build-certificate-trust-bundle.sh" \
  "$source_dir/cert.pem" "$source_dir/chain.pem" "$openvpn_dir/stunnel-ca.crt.next"
chown root:root "$openvpn_dir/stunnel-ca.crt.next"
chmod 0644 "$openvpn_dir/stunnel-ca.crt.next"
mv -f "$stunnel_dir/fullchain.pem.next" "$stunnel_dir/fullchain.pem"
mv -f "$stunnel_dir/privkey.pem.next" "$stunnel_dir/privkey.pem"
mv -f "$openvpn_dir/stunnel-ca.crt.next" "$openvpn_dir/stunnel-ca.crt"

if [[ ${1:-} == --restart ]] && docker compose version >/dev/null 2>&1; then
  ./packaging/server/compose.sh restart edge-proxy hysteria2 openvpn-stunnel
fi

printf 'Ket certificates refreshed.\n'
