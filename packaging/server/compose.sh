#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

exec docker compose \
  --env-file .env \
  --project-name ket \
  -f compose.yaml \
  -f compose.hysteria.yaml \
  -f compose.xray.yaml \
  -f compose.xhttp.yaml \
  -f compose.shadowsocks.yaml \
  -f compose.wireguard.yaml \
  -f compose.openvpn.yaml \
  -f compose.edge.yaml \
  "$@"
