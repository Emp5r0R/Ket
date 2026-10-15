#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

# Each transport overlay runs only when .env enables it (the installer turns
# WireGuard off on kernels without WireGuard support).
enabled() {
  [[ $(sed -n "s/^$1=//p" .env | tail -n 1) == true ]]
}

files=(-f compose.yaml)
enabled KET_HYSTERIA_ENABLED && files+=(-f compose.hysteria.yaml)
enabled KET_XRAY_ENABLED && files+=(-f compose.xray.yaml)
enabled KET_XHTTP_ENABLED && files+=(-f compose.xhttp.yaml)
enabled KET_SHADOWSOCKS_ENABLED && files+=(-f compose.shadowsocks.yaml)
enabled KET_WIREGUARD_ENABLED && files+=(-f compose.wireguard.yaml)
enabled KET_OPENVPN_ENABLED && files+=(-f compose.openvpn.yaml)
files+=(-f compose.edge.yaml)

exec docker compose --env-file .env --project-name ket "${files[@]}" "$@"
