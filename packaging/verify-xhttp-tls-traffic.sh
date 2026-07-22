#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
server_bin=${KET_E2E_SERVER_BIN:-$repo_root/target/debug/ket-server}
xray_bin=${KET_E2E_XRAY_BINARY:-$repo_root/apps/ket-desktop/src-tauri/binaries/xray}
stunnel_bin=${KET_E2E_STUNNEL_BINARY:-$(command -v stunnel || command -v stunnel4 || true)}
target_host=${KET_E2E_XHTTP_TARGET_HOST:-example.com}

if [[ -z ${KET_E2E_SERVER_BIN:-} ]]; then
  cargo build --locked --package ket-server --bin ket-server
fi
[[ -x "$server_bin" ]] || { printf 'Ket server binary is unavailable.\n' >&2; exit 1; }
[[ -x "$xray_bin" ]] || { printf 'Xray is unavailable at %s.\n' "$xray_bin" >&2; exit 1; }
[[ -n "$stunnel_bin" && -x "$stunnel_bin" ]] || { printf 'stunnel is required.\n' >&2; exit 1; }

for dependency in curl jq nc openssl shuf ss; do
  command -v "$dependency" >/dev/null || {
    printf '%s is required for the XHTTP/TLS traffic test.\n' "$dependency" >&2
    exit 1
  }
done
"$xray_bin" version | grep -Fq 'Xray 26.3.27' || {
  printf 'Xray is not the pinned 26.3.27 release.\n' >&2
  exit 1
}

work=$(mktemp -d "${TMPDIR:-/tmp}/ket-xhttp-tls-traffic.XXXXXX")
client_pid= xray_pid= control_pid= carrier_pid=
cleanup() {
  set +e
  for pid in "$client_pid" "$carrier_pid" "$xray_pid" "$control_pid"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
  done
  for pid in "$client_pid" "$carrier_pid" "$xray_pid" "$control_pid"; do
    [[ -n "$pid" ]] && wait "$pid" 2>/dev/null
  done
  if [[ ${KET_E2E_KEEP_WORK:-false} == true ]]; then
    printf 'Retained XHTTP/TLS traffic logs at %s\n' "$work" >&2
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT INT TERM

port_in_use() {
  local port=$1
  [[ -n $(ss -H -ltn "sport = :$port") ]]
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
  printf 'Unable to reserve a local TCP port.\n' >&2
  return 1
}

wait_tcp() {
  local port=$1 label=$2
  for _ in {1..150}; do
    nc -z 127.0.0.1 "$port" >/dev/null 2>&1 && return
    sleep 0.1
  done
  printf '%s did not start.\n' "$label" >&2
  return 1
}

wait_file() {
  local path=$1 label=$2
  for _ in {1..150}; do
    [[ -s "$path" ]] && return
    sleep 0.1
  done
  printf '%s was not created.\n' "$label" >&2
  return 1
}

wait_ready() {
  local url=$1 label=$2
  for _ in {1..200}; do
    curl --fail --silent --show-error "$url" >/dev/null 2>&1 && return
    sleep 0.1
  done
  printf '%s did not become ready.\n' "$label" >&2
  return 1
}

wait_xray_api() {
  for _ in {1..150}; do
    if "$xray_bin" api inboundusercount \
      "--server=127.0.0.1:$api_port" --timeout=1 -tag=vless-xhttp \
      >/dev/null 2>&1; then
      return
    fi
    sleep 0.1
  done
  printf 'Xray API did not become ready.\n' >&2
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
  local uuid=$1 label=$2
  reserve_port
  socks_port=$reserved_port_result
  client_config="$work/xray-client-$label.json"
  jq -n \
    --arg uuid "$uuid" \
    --arg target "$target_host" \
    --arg path "$xhttp_path" \
    --argjson public_port "$public_port" \
    --argjson socks_port "$socks_port" \
    '{
      log:{loglevel:"warning"},
      inbounds:[{
        tag:"ket-socks",listen:"127.0.0.1",port:$socks_port,protocol:"socks",
        settings:{auth:"noauth",udp:true},
        sniffing:{enabled:true,destOverride:["http","tls","quic"],routeOnly:true}
      }],
      outbounds:[{
        tag:"ket-stealth",protocol:"vless",
        settings:{vnext:[{address:"127.0.0.1",port:$public_port,users:[{id:$uuid,encryption:"none"}]}]},
        streamSettings:{
          network:"xhttp",security:"tls",
          tlsSettings:{fingerprint:"chrome",serverName:$target,alpn:["h2","http/1.1"]},
          xhttpSettings:{host:$target,path:$path,mode:"packet-up"}
        }
      }]
    }' > "$client_config"
  chmod 0600 "$client_config"
  jq -e '[.. | objects | select(has("allowInsecure"))] | length == 0' "$client_config" >/dev/null
  SSL_CERT_FILE="$work/ca.crt" "$xray_bin" run -test -c "$client_config" >/dev/null
  SSL_CERT_FILE="$work/ca.crt" "$xray_bin" run -c "$client_config" \
    >"$work/xray-client-$label.log" 2>&1 &
  client_pid=$!
  wait_tcp "$socks_port" "Xray client ($label)"
}

request_target() {
  curl --fail --silent --show-error --max-time 20 --noproxy '' \
    --socks5-hostname "127.0.0.1:$socks_port" "https://$target_host/"
}

assert_target_reachable() {
  local response
  response=$(request_target)
  [[ -n "$response" ]] || {
    printf 'XHTTP/TLS returned an empty HTTPS response.\n' >&2
    return 1
  }
}

assert_target_blocked() {
  if request_target >/dev/null 2>&1; then
    printf 'Revoked XHTTP credentials still carried traffic.\n' >&2
    return 1
  fi
}

wait_traffic() {
  local status
  for _ in {1..100}; do
    status=$(curl --fail --silent --show-error \
      -H "Authorization: Bearer $session_token" \
      "http://127.0.0.1:$control_port/v1/sessions/current")
    if jq -e '.traffic.available == true and ((.traffic.bytes_sent + .traffic.bytes_received) > 0)' \
      <<< "$status" >/dev/null; then
      return
    fi
    sleep 0.1
  done
  printf 'Ket did not report XHTTP traffic counters.\n' >&2
  return 1
}

reserve_port
control_port=$reserved_port_result
reserve_port
api_port=$reserved_port_result
reserve_port
origin_port=$reserved_port_result
reserve_port
public_port=$reserved_port_result
xhttp_path="/ket-xhttp-e2e-$(openssl rand -hex 16)"
runtime_config="$work/xray-server.json"
admin_token=admin-token-for-xhttp-e2e-with-at-least-32-characters
credential_key=credential-key-for-xhttp-e2e-with-at-least-32-characters

openssl req -x509 -newkey rsa:2048 -nodes -days 1 -sha256 \
  -subj '/CN=Ket XHTTP E2E CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -keyout "$work/ca.key" -out "$work/ca.crt" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -sha256 -subj "/CN=$target_host" \
  -keyout "$work/server.key" -out "$work/server.csr" >/dev/null 2>&1
openssl x509 -req -sha256 -days 1 -in "$work/server.csr" \
  -CA "$work/ca.crt" -CAkey "$work/ca.key" -CAcreateserial \
  -extfile <(printf 'subjectAltName=DNS:%s\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n' "$target_host") \
  -out "$work/server.crt" >/dev/null 2>&1
chmod 0600 "$work/ca.key" "$work/server.key"

cat > "$work/stunnel.conf" <<EOF
foreground = yes
pid =
debug = notice
output = $work/stunnel.log

[xhttp]
client = no
accept = 127.0.0.1:$public_port
connect = 127.0.0.1:$origin_port
cert = $work/server.crt
key = $work/server.key
sslVersionMin = TLSv1.2
options = NO_SSLv2
options = NO_SSLv3
EOF
"$stunnel_bin" "$work/stunnel.conf" >"$work/stunnel-stdout.log" 2>&1 &
carrier_pid=$!
wait_tcp "$public_port" 'stunnel TLS carrier'
openssl s_client -connect "127.0.0.1:$public_port" -servername "$target_host" \
  -CAfile "$work/ca.crt" -verify_return_error </dev/null 2>/dev/null |
  grep -Fq 'Verification: OK'

KET_ADMIN_TOKEN="$admin_token" \
KET_PUBLIC_URL="http://127.0.0.1:$control_port" \
KET_BIND="127.0.0.1:$control_port" \
KET_STATE_PATH="$work/state.json" \
KET_NODE_ID=e2e-xhttp-1 KET_NODE_NAME='XHTTP E2E' \
KET_COUNTRY_CODE=IN KET_COUNTRY_NAME=India KET_CITY=Local \
KET_LATITUDE=17.385 KET_LONGITUDE=78.4867 KET_MAX_SESSIONS=2 \
KET_SESSION_TTL_SECONDS=300 KET_TRANSPORTS_JSON='[]' \
KET_XHTTP_ENABLED=true KET_XHTTP_PUBLIC_HOST=127.0.0.1 \
KET_XHTTP_PUBLIC_PORT="$public_port" KET_XHTTP_SNI="$target_host" \
KET_XHTTP_PATH="$xhttp_path" KET_XHTTP_FINGERPRINT=chrome \
KET_XHTTP_LISTEN_HOST=127.0.0.1 KET_XHTTP_LISTEN_PORT="$origin_port" \
KET_XRAY_API_SERVER="127.0.0.1:$api_port" KET_XRAY_API_LISTEN=127.0.0.1 \
KET_XRAY_API_PORT="$api_port" KET_XRAY_BINARY="$xray_bin" \
KET_XRAY_CONFIG_PATH="$runtime_config" KET_XRAY_CREDENTIAL_KEY="$credential_key" \
RUST_LOG=ket_server=warn "$server_bin" >"$work/control.log" 2>&1 &
control_pid=$!
wait_file "$runtime_config" 'Xray server configuration'
jq -e \
  --arg path "$xhttp_path" \
  --argjson port "$origin_port" \
  '.inbounds[] | select(.tag == "vless-xhttp") | .listen == "127.0.0.1" and .port == $port and .streamSettings.network == "xhttp" and .streamSettings.security == "none" and .streamSettings.xhttpSettings == {path:$path,mode:"packet-up"}' \
  "$runtime_config" >/dev/null
"$xray_bin" run -test -c "$runtime_config" >/dev/null
"$xray_bin" run -c "$runtime_config" >"$work/xray-server.log" 2>&1 &
xray_pid=$!
wait_xray_api
wait_ready "http://127.0.0.1:$control_port/readyz" 'Ket control plane'

grant=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $admin_token" -H 'Content-Type: application/json' \
  --data '{"label":"XHTTP TLS traffic","max_connections":2,"valid_for_minutes":60}' \
  "http://127.0.0.1:$control_port/v1/admin/access-grants")
grant_id=$(jq -er '.id' <<< "$grant")
access_code=$(jq -er '.access_code' <<< "$grant")

start_session() {
  local label=$1 session transport
  session=$(curl --fail --silent --show-error -H 'Content-Type: application/json' \
    --data "$(jq -cn --arg code "$access_code" --arg name "$label" '{access_code:$code,client_name:$name}')" \
    "http://127.0.0.1:$control_port/v1/sessions")
  session_token=$(jq -er '.session_token' <<< "$session")
  transport=$(jq -ec '.transports[] | select(.protocol == "stealth")' <<< "$session")
  user_uuid=$(jq -er '.credential.auth' <<< "$transport")
  jq -e \
    --arg path "$xhttp_path" \
    --arg target "$target_host" \
    --argjson port "$public_port" \
    '.endpoint == "127.0.0.1" and .port == $port and .network == "tcp" and .tls_server_name == $target and .options == {encryption:"none",fingerprint:"chrome",mode:"packet-up",path:$path,security:"tls",transport:"xhttp"} and (.credential.auth | length == 36) and (.credential.secrets // {}) == {}' \
    <<< "$transport" >/dev/null
}

start_session 'XHTTP release test'
start_client 00000000-0000-4000-8000-000000000000 wrong-uuid
assert_target_blocked
stop_client
start_client "$user_uuid" release
assert_target_reachable
wait_traffic
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $session_token" \
  "http://127.0.0.1:$control_port/v1/sessions/current" >/dev/null
stop_client
start_client "$user_uuid" released
assert_target_blocked
stop_client

start_session 'XHTTP revocation test'
start_client "$user_uuid" revocation
assert_target_reachable
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $admin_token" \
  "http://127.0.0.1:$control_port/v1/admin/access-grants/$grant_id" >/dev/null
stop_client
start_client "$user_uuid" revoked
assert_target_blocked
stop_client

printf 'Verified XHTTP/TLS traffic: strict TLS, scoped UUID enforcement, counters, release, and grant revocation.\n'
