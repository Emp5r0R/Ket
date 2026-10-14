#!/usr/bin/env bash
# Portable contract tests for the public HTTPS edge renderer and template.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
renderer="$root/packaging/server/render-edge.sh"
template="$root/packaging/server/nginx.conf"
session_proxy="$root/packaging/server/nginx-session-proxy.conf"
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-render-edge.XXXXXX")
trap 'rm -rf "$work"' EXIT

xhttp_path=/assets/0123456789abcdef0123456789abcdef
wireguard_prefix=fedcba9876543210fedcba9876543210

render() {
  local directory=$1
  shift
  env -i PATH="$PATH" KET_EDGE_TEMPLATE="$template" KET_EDGE_RENDER_DIR="$directory" "$@" \
    sh "$renderer" --check
}

expect_render_failure() {
  local label=$1 expected=$2 output
  shift 2
  if output=$(render "$work/rejected" "$@" 2>&1); then
    printf 'FAIL: %s rendered successfully\n' "$label" >&2
    exit 1
  fi
  grep -Fq "$expected" <<<"$output" || {
    printf 'FAIL: %s returned unexpected error: %s\n' "$label" "$output" >&2
    exit 1
  }
}

# A complete deployment renders exact transport paths and the REALITY SNI map.
render "$work/full" KET_XRAY_SNI=www.cloudflare.com \
  KET_XHTTP_PATH="$xhttp_path" KET_WIREGUARD_WS_PATH_PREFIX="$wireguard_prefix"
config="$work/full/nginx.conf"
locations="$work/full/transport-locations.conf"
grep -Fq 'www.cloudflare.com ket_reality;' "$config"
grep -Fq "include $work/full/transport-locations.conf;" "$config"
if grep -Fq '${' "$config"; then
  printf 'FAIL: unrendered template placeholder remains\n' >&2
  exit 1
fi
grep -Fxq "location ~ \"^$xhttp_path(/|\$)\" {" "$locations"
grep -Fxq "location ~ \"^/$wireguard_prefix(/|\$)\" {" "$locations"
grep -Fq 'proxy_pass http://ket_xhttp;' "$locations"
grep -Fq 'proxy_pass http://$ket_wireguard_upstream;' "$locations"
if grep -Eq 'ket-(xhttp|wg)-' "$config" "$locations"; then
  printf 'FAIL: a Ket-branded transport prefix is still routed\n' >&2
  exit 1
fi

# Optional transports render no routes when their overlays are absent.
render "$work/minimal" KET_XRAY_SNI=www.cloudflare.com
[[ ! -s "$work/minimal/transport-locations.conf" ]] || {
  printf 'FAIL: disabled transports rendered routes\n' >&2
  exit 1
}

# Values become nginx syntax, so malformed or injected values fail closed.
expect_render_failure 'missing REALITY SNI' 'KET_XRAY_SNI is required'
expect_render_failure 'REALITY SNI injection' 'KET_XRAY_SNI must be a DNS hostname' \
  'KET_XRAY_SNI=a.example; default ket_reality'
expect_render_failure 'XHTTP path injection' 'KET_XHTTP_PATH must be' \
  KET_XRAY_SNI=www.cloudflare.com 'KET_XHTTP_PATH=/aaaaaaaaaaaaaaaa"{}'
expect_render_failure 'XHTTP short path' 'KET_XHTTP_PATH must be' \
  KET_XRAY_SNI=www.cloudflare.com KET_XHTTP_PATH=/short
expect_render_failure 'XHTTP ambiguous path' 'KET_XHTTP_PATH cannot end in / or contain //' \
  KET_XRAY_SNI=www.cloudflare.com KET_XHTTP_PATH=/aaaaaaaaaaaaaaaa//b
expect_render_failure 'WireGuard prefix slash' 'KET_WIREGUARD_WS_PATH_PREFIX must contain' \
  KET_XRAY_SNI=www.cloudflare.com KET_WIREGUARD_WS_PATH_PREFIX=aaaaaaaaaaaaaaaa/b

# The public vhost proxies the control plane only through the session snippet;
# the only other control-plane route is the container-loopback health listener.
public_vhost=$(sed -n '/listen 127.0.0.1:8448 ssl;/,/^    }$/p' "$template")
if grep -Eq 'proxy_pass|readyz|metrics|node/status|admin|internal' <<<"$public_vhost"; then
  printf 'FAIL: the public vhost exposes an operational route\n' >&2
  exit 1
fi
[[ $(grep -c 'include /etc/nginx/ket/session-proxy.conf;' <<<"$public_vhost") == 2 ]]
grep -Fxq 'proxy_pass http://ket_control;' "$session_proxy"
grep -Fxq 'proxy_intercept_errors on;' "$session_proxy"
grep -Fq 'listen 127.0.0.1:8449;' "$template"

# Unauthenticated rejections are masked; enrolled-client outcomes are not.
masked=$(sed -n 's/^ *error_page \(.*\) = @ket_not_found;$/\1/p' <<<"$public_vhost")
for status in 400 401 404 405 413 415 422; do
  grep -Eq "(^| )$status( |$)" <<<"$masked" || {
    printf 'FAIL: status %s is not masked\n' "$status" >&2
    exit 1
  }
done
for status in 403 409 429 500 502 503 504; do
  if grep -Eq "(^| )$status( |$)" <<<"$masked"; then
    printf 'FAIL: client-visible status %s is masked\n' "$status" >&2
    exit 1
  fi
done

printf 'Edge renderer and template contract tests passed.\n'
