#!/usr/bin/env bash
set -euo pipefail

mock_manager_connection() {
  local request_line line content_length=0 authorization= body= method path response
  IFS= read -r request_line || exit 0
  request_line=${request_line%$'\r'}
  while IFS= read -r line; do
    line=${line%$'\r'}
    [[ -n "$line" ]] || break
    case "$line" in
      [Cc]ontent-[Ll]ength:*) content_length=${line#*: }; ;;
      [Aa]uthorization:*) authorization=${line#*: }; ;;
    esac
  done
  if (( content_length > 0 )); then
    IFS= read -r -N "$content_length" body || true
  fi
  read -r method path _ <<< "$request_line"
  if [[ "$authorization" != "Bearer $KET_E2E_MANAGER_TOKEN" ]]; then
    mock_response 401 '{"code":"unauthorized","message":"manager authentication failed"}'
    return
  fi
  case "$method $path" in
    "GET /healthz") mock_response 204 "" ;;
    "GET /v1/sessions")
      response=$(mock_status_json)
      mock_response 200 "$response"
      ;;
    "PUT /v1/sessions/reconcile")
      mock_reconcile "$body"
      mock_response 204 ""
      ;;
    "POST /v1/sessions/remove")
      mock_remove "$body"
      mock_response 204 ""
      ;;
    *) mock_response 404 '{"code":"not_found","message":"not found"}' ;;
  esac
}

mock_status_rows() {
  local status
  status=$(
    { sleep 0.1; printf 'status 3\n'; sleep 0.2; printf 'quit\n'; } | \
      socat - "TCP:127.0.0.1:$KET_E2E_OPENVPN_MANAGEMENT_PORT" 2>/dev/null || true
  )
  if [[ -n ${KET_E2E_WORK:-} ]]; then
    printf '%s\n' "$status" > "$KET_E2E_WORK/management-status.log"
  fi
  awk -F '\t' '
    $1 == "HEADER" && $2 == "CLIENT_LIST" {
      for (i = 3; i <= NF; i++) column[$i] = i - 1
    }
    $1 == "CLIENT_LIST" && column["Username"] {
      print $(column["Username"]) "\t" $(column["Bytes Received"]) "\t" \
        $(column["Bytes Sent"]) "\t" $(column["Connected Since (time_t)"])
    }
  ' <<< "$status"
}

mock_status_json() {
  local rows username received sent connected json='[]'
  rows=$(mock_status_rows)
  while IFS=$'\t' read -r username received sent connected; do
    [[ -n "$username" ]] || continue
    json=$(jq -cn \
      --argjson current "$json" \
      --arg username "$username" \
      --argjson received "${received:-0}" \
      --argjson sent "${sent:-0}" \
      --argjson connected "${connected:-0}" \
      '$current + [{username:$username,virtual_address:"10.67.0.2",connected_since_epoch_seconds:$connected,bytes_received:$received,bytes_sent:$sent}]')
  done <<< "$rows"
  printf '%s' "$json"
}

mock_kill_username() {
  local username=$1
  [[ "$username" =~ ^[A-Za-z0-9]{12}$ ]] || return 1
  { sleep 0.1; printf 'kill %s\n' "$username"; sleep 0.2; printf 'quit\n'; } | \
    socat - "TCP:127.0.0.1:$KET_E2E_OPENVPN_MANAGEMENT_PORT" >/dev/null 2>&1 || true
}

mock_remove() {
  local body=$1 username
  while IFS= read -r username; do
    mock_kill_username "$username"
  done < <(jq -er '.usernames[]' <<< "$body")
}

mock_reconcile() {
  local body=$1 current username
  current=$(mock_status_json)
  while IFS= read -r username; do
    if ! jq -e --arg username "$username" '.usernames | index($username) != null' \
      <<< "$body" >/dev/null; then
      mock_kill_username "$username"
    fi
  done < <(jq -r '.[].username' <<< "$current")
}

mock_response() {
  local status=$1 body=$2 reason
  case "$status" in
    200) reason=OK ;;
    204) reason='No Content' ;;
    401) reason=Unauthorized ;;
    *) reason='Not Found' ;;
  esac
  printf 'HTTP/1.1 %s %s\r\nContent-Type: application/json\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s' \
    "$status" "$reason" "${#body}" "$body"
}

if [[ ${1:-} == --mock-manager-connection ]]; then
  mock_manager_connection
  exit 0
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
server_bin=${KET_E2E_SERVER_BIN:-$repo_root/target/debug/ket-server}
auth_bin=${KET_E2E_OPENVPN_AUTH_BIN:-$repo_root/target/debug/ket-openvpn-auth}
openvpn_bin=${KET_E2E_OPENVPN_BIN:-$(command -v openvpn || true)}
stunnel_bin=${KET_E2E_STUNNEL_BIN:-$(command -v stunnel || true)}
for dependency in curl jq nc openssl shuf socat; do
  command -v "$dependency" >/dev/null || {
    printf '%s is required for the OpenVPN handshake test.\n' "$dependency" >&2
    exit 1
  }
done
[[ -n "$openvpn_bin" && -x "$openvpn_bin" ]] || { printf 'OpenVPN is required.\n' >&2; exit 1; }
[[ -n "$stunnel_bin" && -x "$stunnel_bin" ]] || { printf 'stunnel is required.\n' >&2; exit 1; }
if [[ -z ${KET_E2E_SERVER_BIN:-} || -z ${KET_E2E_OPENVPN_AUTH_BIN:-} ]]; then
  cargo build --locked --package ket-server --bin ket-server --bin ket-openvpn-auth
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/ket-openvpn-handshake.XXXXXX")
server_pid= outer_pid= manager_pid= control_pid= carrier_pid= client_pid=
cleanup() {
  set +e
  for pid in "$client_pid" "$carrier_pid" "$control_pid" "$manager_pid" "$outer_pid" "$server_pid"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
  done
  for pid in "$client_pid" "$carrier_pid" "$control_pid" "$manager_pid" "$outer_pid" "$server_pid"; do
    [[ -n "$pid" ]] && wait "$pid" 2>/dev/null
  done
  if [[ ${KET_E2E_KEEP_WORK:-false} == true ]]; then
    printf 'Retained OpenVPN handshake logs at %s\n' "$work" >&2
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT INT TERM

reserve_port() {
  local port
  for _ in {1..100}; do
    port=$(shuf -i 20000-50000 -n 1)
    if ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
      printf '%s' "$port"
      return
    fi
  done
  printf 'Unable to reserve a local port.\n' >&2
  return 1
}

wait_port() {
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

wait_connected() {
  local pid=$1 log=$2
  for _ in {1..200}; do
    grep -Fq 'Initialization Sequence Completed' "$log" && return
    kill -0 "$pid" 2>/dev/null || {
      printf 'OpenVPN client exited before connecting.\n' >&2
      sed -n '1,160p' "$log" >&2
      return 1
    }
    sleep 0.1
  done
  printf 'OpenVPN client handshake timed out.\n' >&2
  sed -n '1,160p' "$log" >&2
  return 1
}

wait_disconnected() {
  local pid=$1 log=$2
  for _ in {1..150}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      grep -Eq 'AUTH_FAILED|auth-failure|SIGTERM' "$log" || {
        printf 'OpenVPN exited without a revocation diagnostic.\n' >&2
        return 1
      }
      wait "$pid" 2>/dev/null || true
      return
    fi
    sleep 0.1
  done
  printf 'Revoked OpenVPN client did not terminate.\n' >&2
  sed -n '1,200p' "$log" >&2
  return 1
}

control_port=$(reserve_port)
manager_port=$(reserve_port)
openvpn_port=$(reserve_port)
management_port=$(reserve_port)
outer_port=$(reserve_port)
carrier_port=$(reserve_port)
sni=openvpn.e2e.test
admin_token=admin-token-for-openvpn-e2e-with-at-least-32-characters
manager_token=manager-token-for-openvpn-e2e-with-at-least-32-characters
auth_token=auth-token-for-openvpn-e2e-with-at-least-32-characters

mkdir -p "$work/openvpn" "$work/outer" "$work/state"
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -sha256 \
  -subj '/CN=Ket OpenVPN E2E CA' -addext 'basicConstraints=critical,CA:TRUE' \
  -keyout "$work/openvpn/ca.key" -out "$work/openvpn/ca.crt" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -sha256 -subj "/CN=$sni" \
  -addext "subjectAltName=DNS:$sni" \
  -addext 'keyUsage=critical,digitalSignature,keyEncipherment' \
  -addext 'extendedKeyUsage=serverAuth' \
  -keyout "$work/openvpn/server.key" -out "$work/openvpn/server.csr" >/dev/null 2>&1
openssl x509 -req -days 2 -sha256 -copy_extensions copy \
  -in "$work/openvpn/server.csr" -CA "$work/openvpn/ca.crt" -CAkey "$work/openvpn/ca.key" \
  -CAcreateserial -out "$work/openvpn/server.crt" >/dev/null 2>&1
"$openvpn_bin" --genkey tls-crypt "$work/openvpn/tls-crypt.key"

openssl req -x509 -newkey rsa:2048 -nodes -days 2 -sha256 \
  -subj '/CN=Ket Carrier E2E CA' -addext 'basicConstraints=critical,CA:TRUE' \
  -keyout "$work/outer/ca.key" -out "$work/outer/ca.crt" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -sha256 -subj "/CN=$sni" \
  -addext "subjectAltName=DNS:$sni" -addext 'extendedKeyUsage=serverAuth' \
  -keyout "$work/outer/server.key" -out "$work/outer/server.csr" >/dev/null 2>&1
openssl x509 -req -days 2 -sha256 -copy_extensions copy \
  -in "$work/outer/server.csr" -CA "$work/outer/ca.crt" -CAkey "$work/outer/ca.key" \
  -CAcreateserial -out "$work/outer/server.crt" >/dev/null 2>&1
cat "$work/outer/server.crt" "$work/outer/ca.crt" > "$work/outer/fullchain.pem"
chmod 0600 "$work"/*/*.key

cat > "$work/openvpn-server.conf" <<EOF
local 127.0.0.1
port $openvpn_port
proto tcp-server
dev null
mode server
tls-server
ifconfig-noexec
route-noexec
ca $work/openvpn/ca.crt
cert $work/openvpn/server.crt
key $work/openvpn/server.key
dh none
tls-version-min 1.2
tls-cert-profile preferred
tls-crypt $work/openvpn/tls-crypt.key
verify-client-cert none
username-as-common-name
auth-user-pass-verify $auth_bin via-file
script-security 2
setenv KET_OPENVPN_AUTH_URL http://127.0.0.1:$control_port/internal/v1/openvpn/auth
setenv KET_OPENVPN_AUTH_TOKEN $auth_token
data-ciphers AES-256-GCM:AES-128-GCM:CHACHA20-POLY1305
data-ciphers-fallback AES-256-GCM
auth SHA256
allow-compression no
keepalive 2 10
reneg-sec 0
management 127.0.0.1 $management_port
verb 3
mute 10
EOF

cat > "$work/stunnel-server.conf" <<EOF
foreground = yes
syslog = no
debug = notice
sslVersionMin = TLSv1.2
[ket-openvpn]
accept = 127.0.0.1:$outer_port
connect = 127.0.0.1:$openvpn_port
cert = $work/outer/fullchain.pem
key = $work/outer/server.key
EOF

KET_OPENVPN_AUTH_URL="http://127.0.0.1:$control_port/internal/v1/openvpn/auth" \
KET_OPENVPN_AUTH_TOKEN="$auth_token" \
  "$openvpn_bin" --config "$work/openvpn-server.conf" >"$work/openvpn-server.log" 2>&1 &
server_pid=$!
wait_port "$management_port" 'OpenVPN management'
"$stunnel_bin" "$work/stunnel-server.conf" >"$work/stunnel-server.log" 2>&1 &
outer_pid=$!
wait_port "$outer_port" 'stunnel server'

export KET_E2E_MANAGER_TOKEN=$manager_token
export KET_E2E_OPENVPN_MANAGEMENT_PORT=$management_port
export KET_E2E_WORK=$work
socat "TCP-LISTEN:$manager_port,bind=127.0.0.1,reuseaddr,fork" \
  "EXEC:$repo_root/packaging/verify-openvpn-handshake.sh --mock-manager-connection" \
  >"$work/mock-manager.log" 2>&1 &
manager_pid=$!
wait_port "$manager_port" 'mock OpenVPN manager'

KET_ADMIN_TOKEN="$admin_token" \
KET_PUBLIC_URL="http://127.0.0.1:$control_port" \
KET_BIND="127.0.0.1:$control_port" \
KET_STATE_PATH="$work/state/state.json" \
KET_NODE_ID=e2e-openvpn-1 KET_NODE_NAME='OpenVPN E2E' \
KET_COUNTRY_CODE=IN KET_COUNTRY_NAME=India KET_CITY=Local \
KET_LATITUDE=17.385 KET_LONGITUDE=78.4867 KET_MAX_SESSIONS=4 \
KET_SESSION_TTL_SECONDS=300 KET_TRANSPORTS_JSON='[]' \
KET_OPENVPN_ENABLED=true KET_OPENVPN_PUBLIC_HOST=127.0.0.1 \
KET_OPENVPN_PUBLIC_PORT="$outer_port" KET_OPENVPN_SNI="$sni" \
KET_OPENVPN_MANAGER_URL="http://127.0.0.1:$manager_port" \
KET_OPENVPN_MANAGER_TOKEN="$manager_token" KET_OPENVPN_AUTH_TOKEN="$auth_token" \
KET_OPENVPN_CA_CERT_PATH="$work/openvpn/ca.crt" \
KET_OPENVPN_STUNNEL_CA_CERT_PATH="$work/outer/ca.crt" \
KET_OPENVPN_TLS_CRYPT_KEY_PATH="$work/openvpn/tls-crypt.key" \
RUST_LOG=ket_server=warn "$server_bin" >"$work/control.log" 2>&1 &
control_pid=$!
wait_ready "http://127.0.0.1:$control_port/readyz" 'Ket control plane'

cat > "$work/stunnel-client.conf" <<EOF
foreground = yes
syslog = no
debug = notice
client = yes
sslVersionMin = TLSv1.2
[ket-openvpn]
accept = 127.0.0.1:$carrier_port
connect = 127.0.0.1:$outer_port
verifyChain = yes
CAfile = $work/outer/ca.crt
checkHost = $sni
sni = $sni
EOF
"$stunnel_bin" "$work/stunnel-client.conf" >"$work/stunnel-client.log" 2>&1 &
carrier_pid=$!
wait_port "$carrier_port" 'stunnel client'

grant=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $admin_token" -H 'Content-Type: application/json' \
  --data '{"label":"OpenVPN handshake","max_connections":2,"expires_at_epoch_seconds":null}' \
  "http://127.0.0.1:$control_port/v1/admin/access-grants")
grant_id=$(jq -er '.id' <<< "$grant")
access_code=$(jq -er '.access_code' <<< "$grant")

start_session() {
  local label=$1 session transport
  session=$(curl --fail --silent --show-error -H 'Content-Type: application/json' \
    --data "$(jq -cn --arg code "$access_code" --arg name "$label" '{access_code:$code,client_name:$name}')" \
    "http://127.0.0.1:$control_port/v1/sessions")
  session_token=$(jq -er '.session_token' <<< "$session")
  transport=$(jq -ec '.transports[] | select(.protocol == "open_vpn_stunnel")' <<< "$session")
  username=$(jq -er '.credential.secrets.username' <<< "$transport")
  password=$(jq -er '.credential.auth' <<< "$transport")
  printf '%s\n%s\n' "$username" "$password" > "$work/client-auth"
  jq -er '.credential.secrets.ca_certificate_pem_b64' <<< "$transport" | base64 -d > "$work/client-ca.crt"
  jq -er '.credential.secrets.tls_crypt_key_b64' <<< "$transport" | base64 -d > "$work/client-tls-crypt.key"
  cmp "$work/client-ca.crt" "$work/openvpn/ca.crt"
  sed -n '/^-----BEGIN OpenVPN Static key V1-----$/,/^-----END OpenVPN Static key V1-----$/p' \
    "$work/openvpn/tls-crypt.key" > "$work/expected-tls-crypt.key"
  cmp "$work/client-tls-crypt.key" "$work/expected-tls-crypt.key"
  chmod 0600 "$work/client-auth" "$work/client-tls-crypt.key"
  KET_OPENVPN_AUTH_URL="http://127.0.0.1:$control_port/internal/v1/openvpn/auth" \
  KET_OPENVPN_AUTH_TOKEN="$auth_token" \
    "$auth_bin" "$work/client-auth" || {
      printf 'Scoped credential failed through ket-openvpn-auth before OpenVPN startup.\n' >&2
      return 1
    }
  cat > "$work/openvpn-client.conf" <<EOF
client
dev null
proto tcp-client
remote 127.0.0.1 $carrier_port
nobind
ifconfig-noexec
route-noexec
remote-cert-tls server
verify-x509-name $sni name
tls-version-min 1.2
tls-cert-profile preferred
data-ciphers AES-256-GCM:AES-128-GCM:CHACHA20-POLY1305
data-ciphers-fallback AES-256-GCM
auth SHA256
allow-compression no
auth-retry none
auth-user-pass $work/client-auth
ca $work/client-ca.crt
tls-crypt $work/client-tls-crypt.key
ping 2
ping-restart 10
verb 3
mute 10
EOF
  : > "$work/openvpn-client.log"
  "$openvpn_bin" --config "$work/openvpn-client.conf" >"$work/openvpn-client.log" 2>&1 &
  client_pid=$!
  wait_connected "$client_pid" "$work/openvpn-client.log"
}

start_session 'OpenVPN release test'
sleep 1
status=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $session_token" \
  "http://127.0.0.1:$control_port/v1/sessions/current")
if ! jq -e --arg username "$username" \
  '.session_id == $username and .traffic.available and .traffic.online_connections == 1 and (.traffic.bytes_sent + .traffic.bytes_received > 0)' \
  <<< "$status" >/dev/null; then
  printf 'OpenVPN status did not report the connected session and counters: %s\n' "$status" >&2
  exit 1
fi
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $session_token" \
  "http://127.0.0.1:$control_port/v1/sessions/current" >/dev/null
wait_disconnected "$client_pid" "$work/openvpn-client.log"
client_pid=

start_session 'OpenVPN revocation test'
curl --fail --silent --show-error -X DELETE \
  -H "Authorization: Bearer $admin_token" \
  "http://127.0.0.1:$control_port/v1/admin/access-grants/$grant_id" >/dev/null
wait_disconnected "$client_pid" "$work/openvpn-client.log"
client_pid=

printf 'Verified OpenVPN through certificate-pinned stunnel: scoped auth, handshake counters, release, live kick, and grant revocation.\n'
