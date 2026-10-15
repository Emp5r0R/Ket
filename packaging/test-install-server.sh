#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
installer="$root/packaging/install-server.sh"

automatic=$(
  "$installer" \
    --mode auto \
    --public-ip 8.8.8.8 \
    --email operator@example.com \
    --plan
)
grep -Fq 'Mode: auto' <<<"$automatic"
grep -Fq 'Control hostname: ket-8-8-8-8.sslip.io' <<<"$automatic"
grep -Fq 'DNS: none to configure.' <<<"$automatic"
grep -Fq 'Do NOT expose TCP 8787, 8445, or 8446' <<<"$automatic"
grep -Fq 'TCP+UDP 20000-20031' <<<"$automatic"
grep -Fq 'Git ref: v1.0.1' <<<"$automatic"

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
grep -Fq 'First access-code concurrent-connection limit: 5' <<<"$direct"
grep -Fq 'First access-code lifetime: 1440 minutes' <<<"$direct"
grep -Fq 'DNS: create an A record for ket.example.com' <<<"$direct"

small=$(
  "$installer" \
    --domain ket.example.com \
    --email operator@example.com \
    --max-sessions 1 \
    --plan
)
grep -Fq 'First access-code concurrent-connection limit: 1' <<<"$small"

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
grep -Fq 'ket.example.com = Proxied; direct-ket.example.com = DNS only' <<<"$cloudflare"
grep -Fq 'Cloudflare SSL/TLS: Full (strict)' <<<"$cloudflare"

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

international=$(
  "$installer" \
    --domain ket.example.com \
    --email operator@example.com \
    --country-code CI \
    --country-name "Côte d'Ivoire" \
    --city Abidjan \
    --latitude 5.36 \
    --longitude -4.01 \
    --plan
)
grep -Fq "Location: Abidjan, Côte d'Ivoire (CI; 5.36,-4.01)" <<<"$international"

reject() {
  if "$installer" "$@" --plan >/dev/null 2>&1; then
    printf 'Expected installer arguments to fail: %s\n' "$*" >&2
    exit 1
  fi
}

reject --mode cloudflare --domain ket.example.com --email operator@example.com
reject --mode auto --domain ket.example.com --email operator@example.com --public-ip 8.8.8.8
reject --mode direct --email operator@example.com
reject --mode direct --domain ket.example.com --direct-host raw.example.com --email operator@example.com
reject --mode auto --email operator@example.com --public-ip 999.1.1.1
reject --mode auto --email operator@example.com --public-ip 10.0.0.1
reject --mode auto --email operator@example.com --public-ip 100.64.0.1
reject --mode auto --email operator@example.com --public-ip 192.168.1.1
reject --mode auto --email operator@example.com --public-ip 203.0.113.10
reject --domain bad..example.com --email operator@example.com
reject --domain ket.example.com --email operator@example.com --max-sessions 513
reject --domain ket.example.com --email operator@example.com --first-code-valid-minutes 0
reject --domain ket.example.com --email operator@example.com --first-code-valid-minutes 525601
reject --domain ket.example.com --email operator@example.com --country-name "bad\$name"
reject --domain ket.example.com --email operator@example.com --country-code SG
reject --domain ket.example.com --email operator@example.com --install-dir '/opt/ket server'
reject --domain ket.example.com --email operator@example.com --install-dir /opt/../etc/ket

printf 'Ket installer argument tests passed.\n'

# The installer's node signing key generator must always satisfy the preflight
# and server format (32-byte seed, unpadded Base64URL).
eval "$(sed -n '/^random_base64url_32()/p' "$installer")"
for _ in 1 2 3 4 5 6 7 8; do
  node_key=$(random_base64url_32)
  env -i "PATH=$PATH" \
    KET_ADMIN_TOKEN=01234567890123456789012345678901 \
    KET_PUBLIC_URL=https://ket.example.com \
    "KET_NODE_SIGNING_KEY=$node_key" \
    "$root/packaging/validate-env.sh" >/dev/null
done
# shellcheck disable=SC2016 # literal installer text, not an expansion
grep -Fq 'KET_NODE_SIGNING_KEY=$(random_base64url_32)' "$installer"
grep -Fq "first_invite=\$(jq -er '.invite'" "$installer"
printf 'Installer node-key generation tests passed.\n'
