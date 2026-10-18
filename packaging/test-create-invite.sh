#!/usr/bin/env bash
# Contract tests for packaging/server/create-invite.sh against a stub control API.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
helper="$root/packaging/server/create-invite.sh"
command -v python3 >/dev/null || { printf 'python3 is required for the create-invite tests.\n' >&2; exit 1; }
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-create-invite.XXXXXX")
server_pid=''
cleanup() {
  [[ -z $server_pid ]] || kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

admin_token=test-admin-token-with-at-least-32-characters
invite=ket:invite:STUB_INVITE_VALUE
code=Abcdefghijklmnopqrstuvwxyz012345

# The stub validates the request exactly and answers by requested device count:
# 1-3 succeed (3 omits the invite, like a server without a node key), 9 is refused.
cat > "$work/stub.py" <<EOF
import http.server, json, sys
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/v1/admin/protocols" or self.headers["Authorization"] != "Bearer $admin_token":
            self.send_response(404); self.end_headers(); return
        data = json.dumps([
            {"protocol": "vless_xtls_reality", "name": "reality", "display_name": "VLESS + REALITY"},
            {"protocol": "stealth", "name": "stealth", "display_name": "HTTPS Stealth"},
        ]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        keys = set(body) - {"protocols", "allowance"}
        ok = (self.path == "/v1/admin/access-grants"
              and self.headers["Authorization"] == "Bearer $admin_token"
              and keys == {"label", "max_connections", "valid_for_minutes"})
        if not ok:
            self.send_response(400); self.end_headers(); return
        with open("$work/requests.jsonl", "a") as log:
            log.write(json.dumps(body) + "\n")
        if body["max_connections"] == 9:
            reply, status = {"code": "invalid_request", "message": "max_connections exceeds\u001b[31m capacity"}, 422
        else:
            reply, status = {"access_code": "$code", "expires_at_epoch_seconds": 1900000000}, 201
            if body["max_connections"] != 3:
                reply["invite"] = "$invite"
        data = json.dumps(reply).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def log_message(self, *args):
        pass
server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
print(server.server_address[1], flush=True)
server.serve_forever()
EOF
python3 -I "$work/stub.py" > "$work/port" &
server_pid=$!
for _ in $(seq 1 50); do [[ -s $work/port ]] && break; sleep 0.1; done
port=$(head -n1 "$work/port")
[[ $port =~ ^[0-9]+$ ]] || fail 'stub control API did not start'

mkdir -p "$work/install" "$work/bin"
printf 'KET_ADMIN_TOKEN=%s\nKET_PUBLIC_URL=https://ket.example.com\n' "$admin_token" > "$work/install/.env"
# A stand-in qrencode proves the invite (and only the invite) is rendered as a QR code.
cat > "$work/bin/qrencode" <<'STUB'
#!/usr/bin/env bash
printf 'QR[%s]\n' "${@: -1}"
STUB
chmod +x "$work/bin/qrencode"

run_helper() {
  (cd "$work/install" && env -i "PATH=$work/bin:$PATH" "KET_CONTROL_URL=http://127.0.0.1:$port" "$helper" "$@")
}

output=$(run_helper --label 'Family phones' --devices 2 --days 7)
grep -Fxq "$invite" <<<"$output" || fail 'invite was not printed'
grep -Fxq "QR[$invite]" <<<"$output" || fail 'QR code was not rendered for the invite'
grep -Fq "server https://ket.example.com, access code $code" <<<"$output" || fail 'advanced fallback missing'
grep -Fq 'Up to 2 device(s) at once' <<<"$output" || fail 'device limit missing'
last_request=$(tail -n1 "$work/requests.jsonl")
[[ $last_request == '{"label": "Family phones", "max_connections": 2, "valid_for_minutes": 10080}' ]] \
  || fail "unexpected grant request: $last_request"

# Without KET_CONTROL_URL the helper finds the control plane on .env's
# KET_CONTROL_PORT.
cp "$work/install/.env" "$work/env.saved"
printf 'KET_CONTROL_PORT=%s\n' "$port" >> "$work/install/.env"
output=$(cd "$work/install" && env -i "PATH=$work/bin:$PATH" "$helper" --devices 1 --no-qr) \
  || fail 'the helper did not use KET_CONTROL_PORT from .env'
grep -Fxq "$invite" <<<"$output" || fail 'invite missing when using KET_CONTROL_PORT'
cp "$work/env.saved" "$work/install/.env"

output=$(run_helper --devices 1 --minutes 90 --no-qr)
grep -Fxq "$invite" <<<"$output" || fail 'invite missing with --no-qr'
grep -q 'QR\[' <<<"$output" && fail '--no-qr still rendered a QR code'

output=$(run_helper --devices 3)
grep -Fq 'no node signing key' <<<"$output" || fail 'missing node-key notice for code-only servers'
grep -Fq "Access code: $code" <<<"$output" || fail 'code-only server did not print the access code'
grep -q 'QR\[' <<<"$output" && fail 'a code-only grant must not render a QR code'

# Protocol names are resolved through the server's list, case-insensitively,
# and an invite for every protocol sends no limit at all.
output=$(run_helper --devices 2 --protocols 'Stealth, REALITY' --no-qr)
grep -Fq 'It may use only: stealth, reality' <<<"$output" || fail "protocol limit not reported: $output"
last_request=$(tail -n1 "$work/requests.jsonl")
[[ $last_request == *'"protocols": {"only": ["stealth", "vless_xtls_reality"]}'* ]] \
  || fail "unexpected protocol limit: $last_request"
output=$(run_helper --devices 2 --protocols all --no-qr)
grep -Fq 'It may use only' <<<"$output" && fail 'an all-protocol invite reported a limit'
if output=$(run_helper --devices 2 --protocols wireguard 2>&1); then fail 'an unoffered protocol was accepted'; fi
grep -Fq 'This server offers: reality, stealth' <<<"$output" || fail "unknown protocol not explained: $output"

# A data allowance is sent in bytes and repeated in words; none sends nothing.
output=$(run_helper --devices 2 --data 50GB/month --no-qr)
grep -Fq 'Data: 50 GB a month, shared by its devices (UTC).' <<<"$output" || fail "allowance not reported: $output"
last_request=$(tail -n1 "$work/requests.jsonl")
[[ $last_request == *'"allowance": {"bytes": 53687091200, "period": "month"}'* ]] \
  || fail "unexpected allowance: $last_request"
output=$(run_helper --devices 2 --data none --no-qr)
grep -Fq 'Data:' <<<"$output" && fail 'an invite without an allowance reported one'
[[ $(tail -n1 "$work/requests.jsonl") != *allowance* ]] || fail 'no allowance still sent one'

if output=$(run_helper --devices 9 2>&1); then fail 'refused grant reported success'; fi
grep -Fq 'refused the grant (HTTP 422: max_connections exceeds[31m capacity)' <<<"$output" \
  || fail "refusal reason was not shown safely: $output"

for invalid in '--devices 0' '--devices x' '--days 0' '--minutes 525601' '--label' '--data 5XB/day' '--data' '--bogus'; do
  # shellcheck disable=SC2086 # deliberate word splitting of the option pair
  if run_helper $invalid >/dev/null 2>&1; then fail "accepted invalid arguments: $invalid"; fi
done
if (cd "$work" && env -i "PATH=$PATH" "$helper" >/dev/null 2>&1); then
  fail 'ran outside an install directory without an admin token'
fi
requests=$(wc -l < "$work/requests.jsonl")
((requests == 9)) || fail "invalid invocations reached the server ($requests requests)"

printf 'create-invite contract tests passed.\n'
