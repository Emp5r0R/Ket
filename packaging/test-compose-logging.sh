#!/usr/bin/env bash
# Every container Ket runs keeps small, rotated logs, so a server never
# accumulates connection history about its users.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
status=0
for file in "$root"/compose*.yaml; do
  # A service that sets a restart policy defines a container; overlays that
  # only extend control-plane do not.
  problems=$(awk '
    function check() {
      if (service != "" && restart && !(logging && local && size && files)) print service
    }
    /^services:$/ { in_services = 1; next }
    /^[a-z]/ { check(); in_services = 0; service = "" }
    in_services && /^  [a-z0-9-]+:$/ {
      check()
      service = $1; sub(":", "", service)
      restart = logging = local = size = files = 0
      next
    }
    /^    restart: unless-stopped$/ { restart = 1 }
    /^    logging:$/ { logging = 1 }
    /^      driver: local$/ { local = 1 }
    /^        max-size: "1m"$/ { size = 1 }
    /^        max-file: "3"$/ { files = 1 }
    END { check() }
  ' "$file")
  if [[ -n $problems ]]; then
    printf 'FAIL: %s: services without bounded local logging: %s\n' "${file##*/}" "$(tr '\n' ' ' <<<"$problems")" >&2
    status=1
  fi
done
grep -Fq 'HYSTERIA_LOG_LEVEL: warn' "$root/compose.hysteria.yaml" \
  || { printf 'FAIL: Hysteria2 must not log each client connection\n' >&2; status=1; }
grep -Fxq 'debug = warning' "$root/packaging/openvpn/stunnel-server.conf" \
  || { printf 'FAIL: stunnel must not log each client address\n' >&2; status=1; }
grep -Fq 'error_log /dev/stderr error;' "$root/packaging/server/nginx.conf" \
  || { printf 'FAIL: the edge must log errors only\n' >&2; status=1; }
((status == 0)) && printf 'Compose logging tests passed.\n'
exit "$status"
