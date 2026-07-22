#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
installer="$root/packaging/install-server.sh"

direct=$(
  "$installer" \
    --domain ket.example.com \
    --email operator@example.com \
    --max-sessions 32 \
    --first-code-valid-minutes 1440 \
    --plan
)
grep -Fq 'Mode: direct' <<<"$direct"
grep -Fq 'TCP 80,443,9443,20000-20031' <<<"$direct"
grep -Fq 'Location: automatic public-IP detection' <<<"$direct"
grep -Fq 'First access-code lifetime: 1440 minutes' <<<"$direct"

cloudflare=$(
  "$installer" \
    --mode cloudflare \
    --domain ket.example.com \
    --direct-host direct-ket.example.com \
    --email operator@example.com \
    --max-sessions 64 \
    --plan
)
grep -Fq 'Mode: cloudflare' <<<"$cloudflare"
grep -Fq 'Raw transport hostname: direct-ket.example.com' <<<"$cloudflare"
grep -Fq '20000-20063' <<<"$cloudflare"

manual=$(
  "$installer" \
    --domain ket.example.com \
    --email operator@example.com \
    --country-code SG \
    --country-name Singapore \
    --city Singapore \
    --latitude 1.29 \
    --longitude 103.85 \
    --plan
)
grep -Fq 'Location: Singapore, Singapore (SG; 1.29,103.85)' <<<"$manual"

reject() {
  if "$installer" "$@" --plan >/dev/null 2>&1; then
    printf 'Expected installer arguments to fail: %s\n' "$*" >&2
    exit 1
  fi
}

reject --mode cloudflare --domain ket.example.com --email operator@example.com
reject --domain bad..example.com --email operator@example.com
reject --domain ket.example.com --email operator@example.com --max-sessions 513
reject --domain ket.example.com --email operator@example.com --first-code-valid-minutes 0
reject --domain ket.example.com --email operator@example.com --first-code-valid-minutes 525601
reject --domain ket.example.com --email operator@example.com --country-name 'bad$name'
reject --domain ket.example.com --email operator@example.com --country-code SG

printf 'Ket installer argument tests passed.\n'
