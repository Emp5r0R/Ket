#!/bin/sh
# Render the Ket HTTPS edge configuration from validated deployment values.
#
# Usage: render-edge.sh [--check]
#   Writes $KET_EDGE_RENDER_DIR/nginx.conf and transport-locations.conf, then
#   execs nginx in the foreground unless --check is given. Values are validated
#   before they are written because they become nginx configuration syntax.
set -eu

template=${KET_EDGE_TEMPLATE:-/etc/nginx/templates/ket.conf.template}
render_dir=${KET_EDGE_RENDER_DIR:-/tmp/ket-edge}

fail() {
  printf 'ket-edge: %s\n' "$1" >&2
  exit 1
}

matches() {
  printf '%s' "$1" | grep -Eq "$2"
}

render_transport_locations() {
  if [ -n "${KET_XHTTP_PATH:-}" ]; then
    matches "$KET_XHTTP_PATH" '^/[A-Za-z0-9/_-]{15,127}$' \
      || fail 'KET_XHTTP_PATH must be a 16-128 character absolute path using letters, numbers, /, -, or _'
    case $KET_XHTTP_PATH in
      */ | *//*) fail 'KET_XHTTP_PATH cannot end in / or contain //' ;;
    esac
    cat <<CONF
# XHTTP/TLS Stealth: only the exact deployment path and its subpaths.
location ~ "^${KET_XHTTP_PATH}(/|\$)" {
    # Xray packet-up permits 1 MiB POST bodies by default.
    client_max_body_size 2m;
    proxy_pass http://ket_xhttp;
    proxy_http_version 1.1;
    proxy_request_buffering off;
    proxy_buffering off;
    proxy_read_timeout 1h;
    proxy_send_timeout 1h;
    proxy_set_header Host \$host;
    proxy_set_header Connection "";
    proxy_set_header X-Forwarded-Proto https;
}
CONF
  fi

  if [ -n "${KET_WIREGUARD_WS_PATH_PREFIX:-}" ]; then
    matches "$KET_WIREGUARD_WS_PATH_PREFIX" '^[A-Za-z0-9_-]{16,96}$' \
      || fail 'KET_WIREGUARD_WS_PATH_PREFIX must contain 16-96 letters, numbers, - or _ without slashes'
    cat <<CONF
# WireGuard over WebSocket/TLS: only the exact deployment prefix.
location ~ "^/${KET_WIREGUARD_WS_PATH_PREFIX}(/|\$)" {
    set \$ket_wireguard_upstream wireguard-wstunnel:8080;
    proxy_pass http://\$ket_wireguard_upstream;
    proxy_http_version 1.1;
    proxy_read_timeout 1h;
    proxy_send_timeout 1h;
    proxy_set_header Host \$host;
    proxy_set_header Upgrade \$http_upgrade;
    proxy_set_header Connection \$connection_upgrade;
}
CONF
  fi
}

[ -n "${KET_XRAY_SNI:-}" ] || fail 'KET_XRAY_SNI is required'
matches "$KET_XRAY_SNI" '^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$' \
  || fail 'KET_XRAY_SNI must be a DNS hostname'

mkdir -p "$render_dir"
render_transport_locations > "$render_dir/transport-locations.conf"
sed "s/\${KET_XRAY_SNI}/$KET_XRAY_SNI/g; s#/tmp/ket-edge/#$render_dir/#g" \
  "$template" > "$render_dir/nginx.conf"

if [ "${1:-}" = --check ]; then
  exit 0
fi
exec /usr/sbin/nginx -c "$render_dir/nginx.conf" -g 'daemon off;'
