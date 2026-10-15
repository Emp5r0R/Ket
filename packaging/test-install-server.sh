#!/usr/bin/env bash
# Command-line contract tests for packaging/install-server.sh (plan mode only;
# nothing is installed). Library behaviour is covered by test-installer-lib.sh.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
installer="$root/packaging/install-server.sh"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

plan() {
  "$installer" --public-ip 8.8.8.8 --plan "$@"
}

expect() {
  local output=$1 text=$2
  grep -Fq -- "$text" <<<"$output" || fail "expected '$text' in:"$'\n'"$output"
}

reject() {
  if "$installer" --plan "$@" >/dev/null 2>&1; then
    fail "expected installer arguments to fail: $*"
  fi
}

automatic=$(plan)
expect "$automatic" 'Address: ket-8-8-8-8.sslip.io (automatic, no DNS needed)'
expect "$automatic" 'TCP+UDP 20000-20031'
expect "$automatic" 'Never expose TCP 8787, 8445, or 8446.'
expect "$automatic" 'First invite: up to 5 device(s) for 43200 minutes'
expect "$automatic" 'Install directory: /opt/ket'

direct=$(plan ket.example.com --mode direct --max-sessions 32 --first-code-valid-minutes 1440)
expect "$direct" 'Address: ket.example.com (your domain, pointing straight at this server)'
expect "$direct" 'First invite: up to 5 device(s) for 1440 minutes'

small=$(plan ket.example.com --mode direct --max-sessions 1)
expect "$small" 'First invite: up to 1 device(s)'

cloudflare=$(plan --domain ket.example.com --mode cloudflare --max-sessions 64 --email operator@example.com)
expect "$cloudflare" 'Address: ket.example.com (through Cloudflare)'
expect "$cloudflare" 'Direct address for UDP and raw TLS: ket-8-8-8-8.sslip.io'
expect "$cloudflare" 'TCP+UDP 20000-20063'
expect "$cloudflare" 'Full (strict)'

named=$(plan ket.example.com --mode cloudflare --direct-host direct-ket.example.com)
expect "$named" 'Direct address for UDP and raw TLS: direct-ket.example.com'

plan ket.example.com --mode direct \
  --country-code sg --country-name Singapore --city Singapore --latitude 1.29 --longitude 103.85 >/dev/null
plan ket.example.com --mode direct \
  --country-code CI --country-name "Côte d'Ivoire" --city Abidjan --latitude 5.36 --longitude -4.01 >/dev/null
"$installer" --help | grep -Fq 'Usage: install-server.sh [DOMAIN] [options]' || fail 'help text missing'

reject --public-ip 8.8.8.8 --mode cloudflare
reject --public-ip 8.8.8.8 --mode auto ket.example.com
reject --public-ip 8.8.8.8 --mode direct
reject --public-ip 8.8.8.8 --mode direct ket.example.com --direct-host raw.example.com
reject --public-ip 8.8.8.8 --mode cloudflare ket.example.com --direct-host ket.example.com
reject --public-ip 8.8.8.8 --mode sideways ket.example.com
reject --public-ip 8.8.8.8 --direct-host raw.example.com
for address in 999.1.1.1 10.0.0.1 100.64.0.1 192.168.1.1 203.0.113.10 127.0.0.1; do
  reject --public-ip "$address"
done
reject --public-ip 8.8.8.8 bad..example.com
reject --public-ip 8.8.8.8 one.example.com two.example.com
reject --public-ip 8.8.8.8 --email not-an-email
reject --public-ip 8.8.8.8 --max-sessions 513
reject --public-ip 8.8.8.8 --max-sessions 0
reject --public-ip 8.8.8.8 --first-code-valid-minutes 0
reject --public-ip 8.8.8.8 --first-code-valid-minutes 525601
reject --public-ip 8.8.8.8 --country-name "bad\$name" --country-code SG --city X --latitude 1 --longitude 1
reject --public-ip 8.8.8.8 --country-code SG
reject --public-ip 8.8.8.8 --install-dir '/opt/ket server'
reject --public-ip 8.8.8.8 --install-dir /opt/../etc/ket
reject --public-ip 8.8.8.8 --ref '../main'
reject --public-ip 8.8.8.8 --unknown-option

printf 'Ket installer command-line tests passed.\n'
