#!/usr/bin/env bash
# packaging/server/compose.sh must load only the transport overlays .env enables.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-compose-overlays.XXXXXX")
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/tree/packaging/server" "$work/bin"
cp "$root/packaging/server/compose.sh" "$work/tree/packaging/server/compose.sh"
cat > "$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*"
STUB
chmod +x "$work/bin/docker"

overlays() {
  printf '%s\n' "$@" > "$work/tree/.env"
  PATH="$work/bin:$PATH" "$work/tree/packaging/server/compose.sh" config --quiet
}

all=$(overlays KET_HYSTERIA_ENABLED=true KET_XRAY_ENABLED=true KET_XHTTP_ENABLED=true \
  KET_SHADOWSOCKS_ENABLED=true KET_WIREGUARD_ENABLED=true KET_OPENVPN_ENABLED=true)
expected='compose --env-file .env --project-name ket -f compose.yaml -f compose.hysteria.yaml -f compose.xray.yaml -f compose.xhttp.yaml -f compose.shadowsocks.yaml -f compose.wireguard.yaml -f compose.openvpn.yaml -f compose.edge.yaml config --quiet'
[[ $all == "$expected" ]] || { printf 'FAIL: all overlays: %s\n' "$all" >&2; exit 1; }

without_wireguard=$(overlays KET_HYSTERIA_ENABLED=true KET_XRAY_ENABLED=true KET_XHTTP_ENABLED=true \
  KET_SHADOWSOCKS_ENABLED=true KET_WIREGUARD_ENABLED=false KET_OPENVPN_ENABLED=true)
[[ $without_wireguard != *compose.wireguard.yaml* && $without_wireguard == *compose.openvpn.yaml* ]] \
  || { printf 'FAIL: WireGuard overlay not skipped: %s\n' "$without_wireguard" >&2; exit 1; }

minimal=$(overlays KET_XRAY_ENABLED=true)
[[ $minimal == 'compose --env-file .env --project-name ket -f compose.yaml -f compose.xray.yaml -f compose.edge.yaml config --quiet' ]] \
  || { printf 'FAIL: minimal overlays: %s\n' "$minimal" >&2; exit 1; }

printf 'Compose overlay selection tests passed.\n'
