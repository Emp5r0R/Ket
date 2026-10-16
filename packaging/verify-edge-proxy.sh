#!/usr/bin/env bash
# Runs the pinned edge image with the real rendered configuration in front of
# stub upstreams and probes it the way an active scanner and a client would.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for dependency in curl docker openssl; do
  command -v "$dependency" >/dev/null || {
    printf '%s is required for the edge proxy test.\n' "$dependency" >&2
    exit 1
  }
done

edge_image=$(sed -n 's/^ *image: \(nginx:[^ ]*\)$/\1/p' "$root/compose.edge.yaml")
[[ -n "$edge_image" ]] || { printf 'Edge image is not pinned in compose.edge.yaml.\n' >&2; exit 1; }

run_id=ket-edge-test-$$-$RANDOM
network=$run_id
stub=$run_id-stub
edge=$run_id-edge
host=edge.example.test
xhttp_path=/assets/0123456789abcdef0123456789abcdef
wireguard_prefix=fedcba9876543210fedcba9876543210
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-edge-proxy.XXXXXX")

cleanup() {
  docker rm -f "$edge" "$stub" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  docker logs "$edge" 2>&1 | tail -20 >&2 || true
  exit 1
}

# Stub upstreams answer with the status a real Ket service would return.
cat > "$work/stub.conf" <<'EOF'
events {}
http {
  default_type application/json;
  server {
    listen 8787;
    location = /healthz { return 204; }
    location = /v1/sessions {
      if ($request_method != POST) { return 405 '{"code":"method_not_allowed"}'; }
      # Echo the enrollment nonce back as the node signature so the harness can
      # prove the edge forwards both headers unchanged.
      add_header Ket-Node-Signature $http_ket_enrollment_nonce always;
      if ($http_x_test_outcome = created) { return 201 '{"session_token":"stub"}'; }
      if ($http_x_test_outcome = expired) { return 403 '{"code":"grant_expired"}'; }
      if ($http_x_test_outcome = capacity) { return 409 '{"code":"grant_capacity_reached"}'; }
      if ($http_x_test_outcome = paused) { return 423 '{"code":"grant_paused"}'; }
      if ($http_x_test_outcome = busy) { return 429 '{"code":"server_busy"}'; }
      if ($http_x_test_outcome = unavailable) { return 503 '{"code":"data_plane_unavailable"}'; }
      # An app the server refuses must still see the update details, and the
      # app's Ket-Client header must reach the control plane.
      if ($http_ket_client = "android/0.9.0") { return 426 '{"code":"client_update_required"}'; }
      if ($http_x_test_outcome = malformed) { return 422 '{"code":"invalid_request"}'; }
      return 401 '{"code":"unauthorized","message":"credentials are invalid"}';
    }
    location = /v1/sessions/current {
      if ($http_authorization = "Bearer valid") { return 200 '{"session":"current"}'; }
      return 401 '{"code":"unauthorized","message":"credentials are invalid"}';
    }
    location / { return 200 'ket_active_sessions 1\n'; }
  }
  server { listen 8445; location / { return 200 'xhttp:$request_uri'; } }
  server { listen 8080; location / { return 200 'wireguard:$request_uri'; } }
}
EOF

openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=$host" \
  -addext "subjectAltName=DNS:$host" \
  -keyout "$work/privkey.pem" -out "$work/fullchain.pem" >/dev/null 2>&1
chmod 0644 "$work/privkey.pem" "$work/fullchain.pem" "$work/stub.conf"

docker network create "$network" >/dev/null
docker run -d --name "$stub" --network "$network" \
  --network-alias control-plane --network-alias xray --network-alias wireguard-wstunnel \
  -v "$work/stub.conf:/etc/nginx/nginx.conf:ro" "$edge_image" >/dev/null

docker run -d --name "$edge" --network "$network" -p 127.0.0.1::443 \
  --read-only --tmpfs /var/cache/nginx --tmpfs /var/run --tmpfs /tmp \
  -e KET_XRAY_SNI=www.cloudflare.com \
  -e KET_XHTTP_PATH="$xhttp_path" \
  -e KET_WIREGUARD_WS_PATH_PREFIX="$wireguard_prefix" \
  -v "$root/packaging/server/nginx.conf:/etc/nginx/templates/ket.conf.template:ro" \
  -v "$root/packaging/server/nginx-session-proxy.conf:/etc/nginx/ket/session-proxy.conf:ro" \
  -v "$root/packaging/server/render-edge.sh:/etc/nginx/ket/render-edge.sh:ro" \
  -v "$work:/etc/nginx/tls:ro" \
  --entrypoint /bin/sh "$edge_image" /etc/nginx/ket/render-edge.sh >/dev/null

port=$(docker port "$edge" 443/tcp | sed -n 's/^127\.0\.0\.1:\([0-9]*\)$/\1/p' | head -n1)
[[ -n "$port" ]] || fail 'edge port was not published'

# request METHOD PATH [curl args...] -> "<status> <content-type>" with body in $work/body
request() {
  local method=$1 path=$2
  shift 2
  curl --silent --show-error --insecure --max-time 10 \
    --resolve "$host:$port:127.0.0.1" --request "$method" \
    --output "$work/body" --write-out '%{http_code} %{content_type}' \
    "$@" "https://$host:$port$path"
}

ready=false
for _ in $(seq 1 50); do
  if [[ $(request GET / 2>/dev/null || true) == '200 '* ]]; then
    ready=true
    break
  fi
  sleep 0.2
done
$ready || fail 'edge did not become ready'

expect() {
  local label=$1 expected_status=$2 expected_body=$3 method=$4 path=$5 result
  shift 5
  result=$(request "$method" "$path" "$@") || fail "$label: request failed"
  [[ ${result%% *} == "$expected_status" ]] || fail "$label: expected $expected_status, got $result"
  if [[ -n "$expected_body" ]] && ! grep -Fq -- "$expected_body" "$work/body"; then
    fail "$label: body did not contain $expected_body"
  fi
}

request GET /does-not-exist >/dev/null
cp "$work/body" "$work/decoy-body"
decoy_type=$(request GET /does-not-exist)
[[ $decoy_type == '404 text/html' ]] || fail "decoy 404 has unexpected shape: $decoy_type"
grep -qi 'ket' "$work/decoy-body" && fail 'decoy body names the product'

# Every unauthenticated or operational probe is byte-identical to the decoy.
expect_decoy() {
  local label=$1 result
  shift
  result=$(request "$@") || fail "$label: request failed"
  [[ $result == "$decoy_type" ]] || fail "$label: expected decoy, got $result"
  cmp -s "$work/body" "$work/decoy-body" || fail "$label: body differs from the decoy"
}

oversized=$(head -c 40000 /dev/zero | tr '\0' a)
expect_decoy 'metrics' GET /metrics
expect_decoy 'readiness' GET /readyz
expect_decoy 'health' GET /healthz
expect_decoy 'node status' GET /v1/node/status
expect_decoy 'admin grants' GET /v1/admin/access-grants
expect_decoy 'internal auth' POST /internal/v1/hysteria2/auth
expect_decoy 'session method' GET /v1/sessions
expect_decoy 'invalid access code' POST /v1/sessions --data '{}'
expect_decoy 'malformed enrollment' POST /v1/sessions -H 'X-Test-Outcome: malformed' --data '{}'
expect_decoy 'oversized enrollment' POST /v1/sessions --data "$oversized"
expect_decoy 'invalid session token' GET /v1/sessions/current -H 'Authorization: Bearer forged'
expect_decoy 'legacy XHTTP prefix' GET /ket-xhttp-0123456789abcdef
expect_decoy 'legacy WireGuard prefix' GET /ket-wg-0123456789abcdef/events
expect_decoy 'XHTTP path neighbour' GET "${xhttp_path}0/x"
expect_decoy 'WireGuard prefix neighbour' GET "/${wireguard_prefix}0/events"

# Enrolled-client outcomes and the exact transport routes pass through.
expect 'neutral home page' 200 'under construction' GET /
expect 'enrollment created' 201 'session_token' POST /v1/sessions -H 'X-Test-Outcome: created' --data '{}'
nonce=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8
signature=$(curl --silent --show-error --insecure --max-time 10 \
  --resolve "$host:$port:127.0.0.1" --request POST --data '{}' \
  -H 'X-Test-Outcome: created' -H "Ket-Enrollment-Nonce: $nonce" \
  --dump-header - --output /dev/null "https://$host:$port/v1/sessions" \
  | tr -d '\r' | sed -n 's/^[Kk]et-[Nn]ode-[Ss]ignature: //p')
[[ "$signature" == "$nonce" ]] || fail "edge did not forward the enrollment nonce and node signature headers (got '$signature')"
expect 'grant expired' 403 'grant_expired' POST /v1/sessions -H 'X-Test-Outcome: expired' --data '{}'
expect 'grant capacity' 409 'grant_capacity_reached' POST /v1/sessions -H 'X-Test-Outcome: capacity' --data '{}'
expect 'grant paused' 423 'grant_paused' POST /v1/sessions -H 'X-Test-Outcome: paused' --data '{}'
expect 'server busy' 429 'server_busy' POST /v1/sessions -H 'X-Test-Outcome: busy' --data '{}'
expect 'node unavailable' 503 'data_plane_unavailable' POST /v1/sessions -H 'X-Test-Outcome: unavailable' --data '{}'
expect 'app update required' 426 'client_update_required' POST /v1/sessions -H 'Ket-Client: android/0.9.0' --data '{}'
expect 'session status' 200 'current' GET /v1/sessions/current -H 'Authorization: Bearer valid'
expect 'XHTTP exact path' 200 "xhttp:$xhttp_path" GET "$xhttp_path"
expect 'XHTTP subpath' 200 "xhttp:$xhttp_path/session/0" POST "$xhttp_path/session/0" --data x
expect 'WireGuard events' 200 "wireguard:/$wireguard_prefix/events" GET "/$wireguard_prefix/events"

# The container-loopback health listener reaches the control plane.
docker exec "$edge" wget -q --spider http://127.0.0.1:8449/healthz \
  || fail 'edge health listener did not reach the control plane'

printf 'Edge proxy probe tests passed.\n'
