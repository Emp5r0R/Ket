#!/usr/bin/env bash
set -euo pipefail

http_origin_connection() {
  local request_line line body=${KET_E2E_ORIGIN_BODY:-ket-shadowsocks-e2e}
  IFS= read -r request_line || exit 0
  request_line=${request_line%$'\r'}
  while IFS= read -r line; do
    line=${line%$'\r'}
    [[ -n "$line" ]] || break
  done
  if [[ "$request_line" != "GET /ket-shadowsocks-e2e HTTP/"* ]]; then
    printf 'HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'
    return
  fi
  printf 'HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s' \
    "${#body}" "$body"
}

if [[ ${1:-} == --http-origin-connection ]]; then
  http_origin_connection
  exit 0
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
server_bin=${KET_E2E_SERVER_BIN:-$repo_root/target/debug/ket-server}
if [[ -z ${KET_E2E_SERVER_BIN:-} ]]; then
  cargo build --locked --package ket-server --bin ket-server
fi
[[ -x "$server_bin" ]] || { printf 'Ket server binary is unavailable.\n' >&2; exit 1; }

for dependency in curl jq nc openssl shuf socat ss; do
  command -v "$dependency" >/dev/null || {
    printf '%s is required for the Shadowsocks traffic test.\n' "$dependency" >&2
    exit 1
  }
done

work=$(mktemp -d "${TMPDIR:-/tmp}/ket-shadowsocks-traffic.XXXXXX")
manager_pid= control_pid= origin_pid= client_pid=
cleanup() {
  set +e
  for pid in "$client_pid" "$control_pid" "$origin_pid" "$manager_pid"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
  done
  for pid in "$client_pid" "$control_pid" "$origin_pid" "$manager_pid"; do
    [[ -n "$pid" ]] && wait "$pid" 2>/dev/null
  done
  if [[ ${KET_E2E_KEEP_WORK:-false} == true ]]; then
    printf 'Retained Shadowsocks traffic logs at %s\n' "$work" >&2
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT INT TERM

case "$(uname -m)" in
  x86_64) engine_target=linux-amd64 ;;
  aarch64|arm64) engine_target=linux-arm64 ;;
  *) printf 'Unsupported host architecture for Shadowsocks test: %s\n' "$(uname -m)" >&2; exit 1 ;;
esac
sslocal_bin=${KET_E2E_SHADOWSOCKS_LOCAL_BIN:-$work/sslocal}
ssmanager_bin=${KET_E2E_SHADOWSOCKS_MANAGER_BIN:-$work/ssmanager}
if [[ -z ${KET_E2E_SHADOWSOCKS_LOCAL_BIN:-} ]]; then
  "$repo_root/packaging/fetch-shadowsocks.sh" "$engine_target" "$sslocal_bin" sslocal
fi
if [[ -z ${KET_E2E_SHADOWSOCKS_MANAGER_BIN:-} ]]; then
  "$repo_root/packaging/fetch-shadowsocks.sh" "$engine_target" "$ssmanager_bin" ssmanager
fi
for binary in "$sslocal_bin" "$ssmanager_bin"; do
  [[ -x "$binary" ]] || { printf 'Shadowsocks engine is unavailable at %s.\n' "$binary" >&2; exit 1; }
  "$binary" --version | grep -Fq '1.24.0' || {
    printf 'Shadowsocks engine at %s is not version 1.24.0.\n' "$binary" >&2
    exit 1
  }
done

port_in_use() {
  local port=$1
  [[ -n $(ss -H -ltn "sport = :$port") || -n $(ss -H -lun "sport = :$port") ]]
}

declare -A reserved_ports=()

reserve_port() {
  local port
  for _ in {1..100}; do
    port=$(shuf -i 20000-50000 -n 1)
    if [[ -z ${reserved_ports[$port]:-} ]] && ! port_in_use "$port"; then
      reserved_ports[$port]=1
      reserved_port_result=$port
      return
    fi
  done
  printf 'Unable to reserve a local port.\n' >&2
  return 1
}

reserve_port_pair() {
  local port
  for _ in {1..100}; do
    port=$(shuf -i 20000-49999 -n 1)
    if [[ -z ${reserved_ports[$port]:-} && -z ${reserved_ports[$((port + 1))]:-} ]] &&
      ! port_in_use "$port" && ! port_in_use "$((port + 1))"; then
      reserved_ports[$port]=1
      reserved_ports[$((port + 1))]=1
      reserved_port_result=$port
      return
    fi
  done
  printf 'Unable to reserve a local port pair.\n' >&2
  return 1
}

wait_tcp() {
  local port=$1 label=$2
  for _ in {1..100}; do
    nc -z 127.0.0.1 "$port" >/dev/null 2>&1 && return
    sleep 0.1
  done
  printf '%s did not start.\n' "$label" >&2
  return 1
}

wait_ready() {
  local url=$1 label=$2
  for _ in {1..150}; do
    curl --fail --silent --show-error "$url" >/dev/null 2>&1 && return
    sleep 0.1
  done
  printf '%s did not become ready.\n' "$label" >&2
  return 1
}

manager_request() {
  local command=$1
  printf '%s\n' "$command" | socat -T 2 - "UDP:127.0.0.1:$manager_port"
}

manager_has_port() {
  local port=$1 response payload
  response=$(manager_request ping)
  payload=${response#stat: }
  jq -e --arg port "$port" 'has($port)' <<< "$payload" >/dev/null
}

wait_manager() {
  for _ in {1..100}; do
    manager_request ping 2>/dev/null | grep -Fq 'stat:' && return
    sleep 0.1
  done
  printf 'Shadowsocks manager did not start.\n' >&2
  return 1
}

wait_port_removed() {
  local port=$1
  for _ in {1..100}; do
    if ! manager_has_port "$port"; then
      return
    fi
    sleep 0.1
  done
  printf 'Shadowsocks manager retained revoked port %s.\n' "$port" >&2
  return 1
}

wait_nonzero_counter() {
  local port=$1 response payload
  for _ in {1..100}; do
    response=$(manager_request ping)
    payload=${response#stat: }
    if jq -e --arg port "$port" '(.[$port] // 0) > 0' <<< "$payload" >/dev/null; then
      return
    fi
    sleep 0.1
  done
  printf 'Shadowsocks manager did not report traffic for port %s.\n' "$port" >&2
  return 1
}

stop_client() {
  if [[ -n "$client_pid" ]]; then
    kill "$client_pid" 2>/dev/null || true
    wait "$client_pid" 2>/dev/null || true
    client_pid=
  fi
}

start_client() {
  local key=$1 label=$2
  reserve_port
  socks_port=$reserved_port_result
  client_config="$work/sslocal-$label.json"
  jq -n \
    --arg server 127.0.0.1 \
    --argjson server_port "$relay_port" \
    --argjson local_port "$socks_port" \
    --arg password "$key" \
    '{server:$server,server_port:$server_port,local_address:"127.0.0.1",local_port:$local_port,password:$password,method:"2022-blake3-aes-256-gcm",mode:"tcp_and_udp",timeout:300,udp_timeout:300,no_delay:true,keep_alive:30}' \
    > "$client_config"
  chmod 0600 "$client_config"
  "$sslocal_bin" --config "$client_config" >"$work/sslocal-$label.log" 2>&1 &
  client_pid=$!
  wait_tcp "$socks_port" "Shadowsocks client ($label)"
}

request_origin() {
  curl --fail --silent --show-error --max-time 5 --noproxy '' \
    --socks5-hostname "127.0.0.1:$socks_port" \
    "http://127.0.0.1:$origin_port/ket-shadowsocks-e2e"
}

assert_origin_reachable() {
  local response
  response=$(request_origin)
  [[ "$response" == "$KET_E2E_ORIGIN_BODY" ]] || {
    printf 'Shadowsocks returned an unexpected origin response.\n' >&2
    return 1
  }
}

assert_origin_blocked() {
  if request_origin >/dev/null 2>&1; then
    printf 'Revoked Shadowsocks credentials still carried traffic.\n' >&2
    return 1
  fi
}

reserve_port
manager_port=$reserved_port_result
reserve_port_pair
relay_start=$reserved_port_result
reserve_port
control_port=$reserved_port_result
reserve_port
origin_port=$reserved_port_result
export KET_E2E_ORIGIN_BODY="ket-shadowsocks-e2e-$(openssl rand -hex 16)"
admin_token=admin-token-for-shadowsocks-e2e-with-at-least-32-characters
credential_key=credential-key-for-shadowsocks-e2e-with-at-least-32-characters

"$ssmanager_bin" -U \
  --server-host 127.0.0.1 \
  --manager-addr "127.0.0.1:$manager_port" \
  --encrypt-method 2022-blake3-aes-256-gcm \
  --udp-timeout 300 --udp-max-associations 256 \
  --tcp-no-delay --tcp-keep-alive 30 --worker-threads 1 \
  >"$work/ssmanager.log" 2>&1 &
manager_pid=$!
wait_manager

socat "TCP-LISTEN:$origin_port,bind=127.0.0.1,reuseaddr,fork" \
  "EXEC:$repo_root/packaging/verify-shadowsocks-traffic.sh --http-origin-connection" \
  >"$work/origin.log" 2>&1 &
origin_pid=$!
wait_tcp "$origin_port" 'HTTP origin'

KET_ADMIN_TOKEN="$admin_token" \
KET_PUBLIC_URL="http://127.0.0.1:$control_port" \
KET_BIND="127.0.0.1:$control_port" \
KET_STATE_PATH="$work/state.json" \
KET_NODE_ID=e2e-shadowsocks-1 KET_NODE_NAME='Shadowsocks E2E' \
KET_COUNTRY_CODE=IN KET_COUNTRY_NAME=India KET_CITY=Local \
KET_LATITUDE=17.385 KET_LONGITUDE=78.4867 KET_MAX_SESSIONS=2 \
KET_SESSION_TTL_SECONDS=300 KET_TRANSPORTS_JSON='[]' \
KET_SHADOWSOCKS_ENABLED=true KET_SHADOWSOCKS_PUBLIC_HOST=127.0.0.1 \
KET_SHADOWSOCKS_PORT_START="$relay_start" KET_SHADOWSOCKS_PORT_END="$((relay_start + 1))" \
KET_SHADOWSOCKS_MANAGER_ADDRESS="127.0.0.1:$manager_port" \
KET_SHADOWSOCKS_CREDENTIAL_KEY="$credential_key" \
RUST_LOG=ket_server=warn "$server_bin" >"$work/control.log" 2>&1 &
control_pid=$!
wait_ready "http://127.0.0.1:$control_port/readyz" 'Ket control plane'

grant=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $admin_token" -H 'Content-Type: application/json' \
  --data '{"label":"Shadowsocks traffic","max_connections":2,"expires_at_epoch_seconds":null}' \
  "http://127.0.0.1:$control_port/v1/admin/access-grants")
grant_id=$(jq -er '.id' <<< "$grant")
access_code=$(jq -er '.access_code' <<< "$grant")

start_session() {
  local label=$1 session transport
  session=$(curl --fail --silent --show-error -H 'Content-Type: application/json' \
    --data "$(jq -cn --arg code "$access_code" --arg name "$label" '{access_code:$code,client_name:$name}')" \
    "http://127.0.0.1:$control_port/v1/sessions")
  session_token=$(jq -er '.session_token' <<< "$session")
  transport=$(jq -ec '.transports[] | select(.protocol == "shadowsocks2022")' <<< "$session")
  relay_port=$(jq -er '.port' <<< "$transport")
  password=$(jq -er '.credential.auth' <<< "$transport")
  jq -e \
    '.endpoint == "127.0.0.1" and .network == "tcp_and_udp" and .tls_server_name == null and .options == {method:"2022-blake3-aes-256-gcm",mode:"tcp_and_udp",port_allocation:"lease_slot"} and (.credential.auth | length == 44) and (.credential.secrets // {}) == {}' \
    <<< "$transport" >/dev/null
  manager_has_port "$relay_port" || {
    printf 'Ket did not provision Shadowsocks relay port %s.\n' "$relay_port" >&2
    return 1
  }
}

start_session 'Shadowsocks release test'
wrong_password=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
start_client "$wrong_password" wrong-key
assert_origin_blocked
stop_client
start_client "$password" release
assert_origin_reachable
wait_nonzero_counter "$relay_port"
status=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $session_token" \
  "http://127.0.0.1:$control_port/v1/sessions/current")
jq -e \
  '.traffic.available == false and .traffic.bytes_sent == 0 and .traffic.bytes_received == 0 and .traffic.online_connections == 0' \
  <<< "$status" >/dev/null
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $session_token" \
  "http://127.0.0.1:$control_port/v1/sessions/current" >/dev/null
wait_port_removed "$relay_port"
assert_origin_blocked
stop_client

start_session 'Shadowsocks revocation test'
start_client "$password" revocation
assert_origin_reachable
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $admin_token" \
  "http://127.0.0.1:$control_port/v1/admin/access-grants/$grant_id" >/dev/null
wait_port_removed "$relay_port"
assert_origin_blocked
stop_client

printf 'Verified Shadowsocks 2022 TCP traffic: scoped key enforcement, counters, truthful telemetry, release, and grant revocation.\n'
