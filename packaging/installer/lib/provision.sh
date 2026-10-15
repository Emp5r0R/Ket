# shellcheck shell=bash
# Secrets, configuration, and first start for the Ket server installer.
# Sourced, never executed. Runs inside the staged source directory.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

readonly XRAY_IMAGE='ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7'

# Prints "PRIVATE PUBLIC" as unpadded Base64URL X25519 keys from the pinned
# Xray image, so the host needs no WireGuard or Xray tools.
x25519_key_pair() {
  local output private public
  output=$(docker run --rm "$XRAY_IMAGE" x25519)
  private=$(sed -n 's/^PrivateKey: //p' <<<"$output")
  public=$(sed -n 's/^Password (PublicKey): //p' <<<"$output")
  [[ $private =~ ^[A-Za-z0-9_-]{43}$ && $public =~ ^[A-Za-z0-9_-]{43}$ ]] \
    || fail 'Xray did not return a valid X25519 key pair'
  printf '%s %s' "$private" "$public"
}

write_environment_file() {
  local reality wireguard xray_private xray_public wireguard_private wireguard_public node_id
  reality=$(x25519_key_pair)
  xray_private=${reality% *}
  xray_public=${reality#* }
  wireguard=$(x25519_key_pair)
  wireguard_private=$(base64url_key_to_standard "${wireguard% *}")
  wireguard_public=$(base64url_key_to_standard "${wireguard#* }")
  node_id=$(printf '%s-%s' "${country_code,,}" "$direct_host" | tr -c 'a-z0-9._-' '-' | cut -c1-64)
  cat > .env <<EOF
KET_ADMIN_TOKEN=$(random_secret)
KET_PUBLIC_URL=https://$domain
# Node identity for signed invites. Never rotate it: that invalidates every invite.
KET_NODE_SIGNING_KEY=$(random_base64url_32)
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
KET_XHTTP_PATH=/assets/$(random_hex 16)
KET_XHTTP_FINGERPRINT=chrome
KET_XHTTP_ORIGIN_BIND_ADDRESS=127.0.0.1
KET_XHTTP_ORIGIN_PORT=8445

KET_SHADOWSOCKS_ENABLED=true
KET_SHADOWSOCKS_PUBLIC_HOST=$direct_host
KET_SHADOWSOCKS_PORT_START=$shadowsocks_start
KET_SHADOWSOCKS_PORT_END=$shadowsocks_end
KET_SHADOWSOCKS_CREDENTIAL_KEY=$(random_secret)

# Disabled automatically when the kernel has no WireGuard support.
KET_WIREGUARD_ENABLED=$wireguard_enabled
KET_WIREGUARD_PUBLIC_HOST=$domain
KET_WIREGUARD_PUBLIC_PORT=443
KET_WIREGUARD_SNI=$domain
KET_WIREGUARD_WS_PATH_PREFIX=$(random_hex 16)
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
}

create_openvpn_pki() {
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
}

build_and_validate() {
  ./packaging/server/refresh-certificates.sh
  docker build --tag ket-control-plane:local .
  docker run --rm --user 0:0 \
    --entrypoint /usr/local/bin/openvpn \
    -v "$PWD/secrets/openvpn:/out" \
    ket-control-plane:local --genkey tls-crypt /out/tls-crypt.key
  chmod 0644 secrets/openvpn/tls-crypt.key
  set -a
  # shellcheck source=/dev/null
  . ./.env
  set +a
  ./packaging/validate-env.sh
  ./packaging/server/compose.sh config --quiet
}

wait_until_ready() {
  local attempt
  for attempt in $(seq 1 60); do
    if curl --fail --silent --show-error http://127.0.0.1:8787/readyz >/dev/null 2>&1 \
      && curl --fail --silent --show-error \
        --resolve "$domain:443:127.0.0.1" "https://$domain/" >/dev/null 2>&1; then
      return 0
    fi
    ((attempt < 60)) && sleep 2
  done
  ./packaging/server/compose.sh ps
  fail 'Ket did not become ready within 2 minutes'
}
