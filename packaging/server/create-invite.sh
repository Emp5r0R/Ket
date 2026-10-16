#!/usr/bin/env bash
# Create one access grant on this Ket server and print its invite.
#
# Run from the Ket install directory (default /opt/ket) as root:
#   sudo ./packaging/server/create-invite.sh --label "Family" --devices 3 --days 30
#
# The invite and access code are bearer secrets: share them only with the
# people who should connect. The server never stores their plaintext.
set -euo pipefail

# shellcheck source=packaging/server/cli/protocols.sh
. "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")/cli/protocols.sh"

label="Ket devices"
devices=1
minutes=$((30 * 24 * 60))
protocols=all
show_qr=true

usage() {
  cat <<'EOF'
Usage: create-invite.sh [--label TEXT] [--devices N] [--days N | --minutes N]
                        [--protocols LIST] [--no-qr]

  --label TEXT   Name for this grant in the admin list (default: "Ket devices").
  --devices N    How many devices may be connected at the same time (default: 1).
  --days N       Access lifetime in days (default: 30).
  --minutes N    Access lifetime in minutes (1-525600), instead of --days.
  --protocols L  "all" (default), or the protocols it may use, such as
                 "stealth reality" (sudo ket invites protocols lists them).
  --no-qr        Do not print a terminal QR code.
EOF
}

fail() {
  printf 'create-invite: %s\n' "$1" >&2
  exit 1
}

positive_integer() {
  [[ $1 =~ ^[1-9][0-9]{0,6}$ ]]
}

while (($#)); do
  case $1 in
    --label) [[ $# -ge 2 ]] || fail '--label needs a value'; label=$2; shift 2 ;;
    --devices) [[ $# -ge 2 ]] || fail '--devices needs a value'; devices=$2; shift 2 ;;
    --days)
      if [[ $# -lt 2 ]] || ! positive_integer "$2"; then fail '--days must be a positive whole number'; fi
      minutes=$(($2 * 24 * 60)); shift 2 ;;
    --minutes) [[ $# -ge 2 ]] || fail '--minutes needs a value'; minutes=$2; shift 2 ;;
    --protocols) [[ $# -ge 2 ]] || fail '--protocols needs a value'; protocols=$2; shift 2 ;;
    --no-qr) show_qr=false; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; fail "unknown option: $1" ;;
  esac
done

[[ -n $label && ${#label} -le 128 && ! $label =~ [[:cntrl:]] ]] || fail '--label must be 1-128 printable characters'
positive_integer "$devices" || fail '--devices must be a positive whole number'
if ! positive_integer "$minutes" || ((minutes > 525600)); then
  fail 'access lifetime must be 1-525600 minutes (365 days)'
fi

if [[ -z ${KET_ADMIN_TOKEN:-} ]]; then
  [[ -r .env ]] || fail 'run this from the Ket install directory (for example /opt/ket) as root'
  KET_ADMIN_TOKEN=$(sed -n 's/^KET_ADMIN_TOKEN=//p' .env | tail -n 1)
fi
[[ -n $KET_ADMIN_TOKEN ]] || fail 'KET_ADMIN_TOKEN is not configured'
# The control plane listens on the host port the installation chose
# (KET_CONTROL_PORT in .env), not always 8787.
control_port=${KET_CONTROL_PORT:-}
if [[ -z $control_port && -r .env ]]; then
  control_port=$(sed -n 's/^KET_CONTROL_PORT=//p' .env | tail -n 1)
fi
control_url=${KET_CONTROL_URL:-http://127.0.0.1:${control_port:-8787}}
for dependency in curl jq; do
  command -v "$dependency" >/dev/null || fail "$dependency is required"
done

# admin_get PATH: the body of a loopback admin GET, or nothing on failure.
admin_get() {
  curl --silent --show-error --max-time 30 --fail \
    --header "Authorization: Bearer $KET_ADMIN_TOKEN" "$control_url$1"
}

access='"all"'
if [[ ${protocols,,} != all ]]; then
  offered=$(admin_get /v1/admin/protocols) || fail 'could not ask Ket which protocols it offers'
  access=$(protocol_access_json "$protocols" "$offered") || exit 1
fi

reply=$(
  jq -n --arg label "$label" --argjson max_connections "$devices" --argjson valid_for_minutes "$minutes" \
    --argjson protocols "$access" \
    '{label:$label,max_connections:$max_connections,valid_for_minutes:$valid_for_minutes}
     + (if $protocols == "all" then {} else {protocols:$protocols} end)' \
    | curl --silent --show-error --max-time 30 \
        --request POST \
        --header "Authorization: Bearer $KET_ADMIN_TOKEN" \
        --header 'Content-Type: application/json' \
        --data-binary @- \
        --write-out '\n%{http_code}' \
        "$control_url/v1/admin/access-grants"
) || fail 'could not reach Ket on this server (check: ./packaging/server/compose.sh ps)'
status=${reply##*$'\n'}
response=${reply%$'\n'*}
if [[ $status != 201 ]]; then
  reason=$(jq -r '.message // empty' <<<"$response" 2>/dev/null | tr -d '[:cntrl:]' | cut -c1-200)
  fail "the server refused the grant (HTTP $status${reason:+: $reason})"
fi

access_code=$(jq -er '.access_code' <<<"$response") || fail 'the server response had no access code'
expires_at=$(jq -er '.expires_at_epoch_seconds' <<<"$response") || fail 'the server response had no expiry'
invite=$(jq -r '.invite // empty' <<<"$response")
server_url=$(sed -n 's/^KET_PUBLIC_URL=//p' .env 2>/dev/null | tail -n 1)
expiry_text=$(date -u -d "@$expires_at" '+%Y-%m-%d %H:%M UTC' 2>/dev/null || printf 'Unix time %s' "$expires_at")

if [[ -n $invite ]]; then
  printf '\nInvite (paste it, or scan the QR code, in the Ket app):\n%s\n' "$invite"
  if $show_qr && command -v qrencode >/dev/null 2>&1; then
    printf '\n'
    qrencode -t ANSIUTF8 -m 2 -l M "$invite"
  fi
  printf '\nAdvanced (older apps): server %s, access code %s\n' "${server_url:-<this server>}" "$access_code"
else
  printf '\nThis server has no node signing key, so it issues access codes only.\n'
  printf 'Server: %s\nAccess code: %s\n' "${server_url:-<this server>}" "$access_code"
fi
printf 'Up to %s device(s) at once; access ends %s.\n' "$devices" "$expiry_text"
if [[ $access != '"all"' ]]; then
  printf 'It may use only: %s\n' "$(protocol_names "$access" "$offered")"
fi
