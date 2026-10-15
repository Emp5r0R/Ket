#!/usr/bin/env bash
# Tests for the root install.sh bootstrap against a local stand-in for GitHub's
# latest-release redirect and source tarballs.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bootstrap="$root/install.sh"
command -v python3 >/dev/null || { printf 'python3 is required for the bootstrap tests.\n' >&2; exit 1; }
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-bootstrap.XXXXXX")
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

# A current release: protocol 2 and an installer stub that reports how it ran.
mkdir -p "$work/src/Ket-9.9.9/packaging/installer" "$work/src/Ket-1.0.1/packaging" "$work/tarballs"
printf '2\n' > "$work/src/Ket-9.9.9/packaging/installer/protocol"
cat > "$work/src/Ket-9.9.9/packaging/install-server.sh" <<'STUB'
#!/usr/bin/env bash
printf 'ARGS:%s\n' "$*"
[[ -f $2/packaging/installer/protocol ]] && printf 'TREE:ok\n'
rm -rf -- "$2"
STUB
# An older release without one-command support.
printf '#!/usr/bin/env bash\nexit 0\n' > "$work/src/Ket-1.0.1/packaging/install-server.sh"
tar -C "$work/src" -czf "$work/tarballs/v9.9.9" Ket-9.9.9
tar -C "$work/src" -czf "$work/tarballs/v1.0.1" Ket-1.0.1

cat > "$work/stub.py" <<EOF
import http.server, os
TARBALLS = "$work/tarballs"
class Handler(http.server.BaseHTTPRequestHandler):
    def route(self, body):
        if self.path == "/Emp5r0R/Ket/releases/latest":
            self.send_response(302)
            self.send_header("Location", "/Emp5r0R/Ket/releases/tag/v9.9.9")
            self.end_headers()
            return
        if self.path == "/Emp5r0R/Ket/releases/tag/v9.9.9":
            data = b"release page"
        elif self.path.startswith("/Emp5r0R/Ket/tar.gz/"):
            name = os.path.basename(self.path)
            path = os.path.join(TARBALLS, name)
            if not os.path.isfile(path):
                self.send_response(404); self.end_headers(); return
            data = open(path, "rb").read()
        else:
            self.send_response(404); self.end_headers(); return
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if body:
            self.wfile.write(data)
    def do_GET(self):
        self.route(True)
    def do_HEAD(self):
        self.route(False)
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
[[ $port =~ ^[0-9]+$ ]] || fail 'stub GitHub did not start'
base="http://127.0.0.1:$port"

run_bootstrap() {
  env "KET_GITHUB_URL=$base" "KET_CODELOAD_URL=$base" "TMPDIR=$work" bash "$bootstrap" "$@"
}

output=$(run_bootstrap vpn.example.com --plan)
grep -Fq 'Downloading Ket v9.9.9' <<<"$output" || fail 'latest release was not resolved'
grep -Eq "^ARGS:--source-dir $work/ket-setup\.[A-Za-z0-9]+ --ref v9\.9\.9 vpn\.example\.com --plan$" <<<"$output" \
  || fail "installer was not run with the downloaded tree: $output"
grep -Fxq 'TREE:ok' <<<"$output" || fail 'installer did not receive the extracted release tree'

if output=$(KET_REF=v1.0.1 run_bootstrap 2>&1); then fail 'a pre-protocol release ran'; fi
grep -Fq 'Ket v1.0.1 was released before one-command setup' <<<"$output" || fail "missing legacy notice: $output"
grep -Fq "Emp5r0R/Ket/v1.0.1/packaging/install-server.sh" <<<"$output" || fail 'legacy command does not match the release'

if output=$(KET_REF=v0.0.0 run_bootstrap 2>&1); then fail 'a missing release was accepted'; fi
grep -Fq 'could not download Ket v0.0.0' <<<"$output" || fail "missing download error: $output"
if KET_REF='../main' run_bootstrap >/dev/null 2>&1; then fail 'an unsafe release name was accepted'; fi

leftovers=$(find "$work" -maxdepth 1 -name 'ket-setup.*' | wc -l)
((leftovers == 0)) || fail 'the bootstrap left downloaded trees behind'

printf 'Ket bootstrap tests passed.\n'
