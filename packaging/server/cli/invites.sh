# shellcheck shell=bash
# Invites and access grants: create (with an interactive wizard), list, and
# revoke. Sourced by the ket command; never executed.
# shellcheck disable=SC2034,SC2154

# admin_request METHOD PATH: calls the loopback admin API and prints the body
# followed by a final line holding the HTTP status.
admin_request() {
  local token port
  token=$(env_value KET_ADMIN_TOKEN)
  port=$(env_value KET_CONTROL_PORT)
  [[ -n $token ]] || fail 'KET_ADMIN_TOKEN is missing from .env'
  curl --silent --show-error --max-time 30 --request "$1" \
    --header "Authorization: Bearer $token" \
    --write-out '\n%{http_code}' \
    "http://127.0.0.1:${port:-8787}$2" \
    || fail 'could not reach Ket on this server; check: sudo ket status'
}

# ask_number PROMPT DEFAULT MAX: asks until the answer is a whole number 1-MAX.
ask_number() {
  local answer attempt
  for attempt in 1 2 3; do
    answer=$(ask "$1" "$2")
    if [[ $answer =~ ^[1-9][0-9]{0,5}$ ]] && ((answer <= $3)); then
      printf '%s' "$answer"
      return 0
    fi
    printf 'Please enter a whole number from 1 to %s.\n' "$3" >> "$(ket_tty)"
  done
  fail 'no valid answer given'
}

invite_usage() {
  cat <<'USAGE'
Usage: ket invite [--label TEXT] [--devices N] [--days N | --minutes N] [--no-qr]

Without options on a terminal, ket asks who the invite is for, how many
devices may connect at once, and for how many days.
USAGE
}

cmd_invite() {
  case ${1:-} in -h|--help) invite_usage; return 0 ;; esac
  require_root
  require_installation
  if (($# == 0)) && has_terminal; then
    local label devices days
    info 'Create an invite. Press Enter to accept the suggestion in brackets.'
    label=$(ask 'Who is it for (a name you will recognise later)' 'Ket devices')
    devices=$(ask_number 'How many devices may be connected at the same time' 1 512)
    days=$(ask_number 'For how many days should it work' 30 365)
    set -- --label "$label" --devices "$devices" --days "$days"
  fi
  (cd "$install_dir" && ./packaging/server/create-invite.sh "$@")
}

cmd_invites() {
  require_root
  require_installation
  require_commands curl jq
  local reply status body
  reply=$(admin_request GET /v1/admin/access-grants)
  status=${reply##*$'\n'}
  body=${reply%$'\n'*}
  [[ $status == 200 ]] || fail "the server refused to list invites (HTTP $status)"
  if [[ $(jq 'length' <<<"$body") == 0 ]]; then
    info 'No invites yet. Create one with: sudo ket invite'
    return 0
  fi
  jq -r --argjson now "$(date +%s)" '
    ["ID", "FOR", "IN USE", "EXPIRES"],
    (sort_by(.created_at_epoch_seconds) | reverse | .[]
      | [ .id,
          (.label | gsub("[[:cntrl:]]"; "") | .[0:32]),
          "\(.active_connections)/\(.max_connections)",
          (if .expires_at_epoch_seconds == null then "never"
           elif .expires_at_epoch_seconds <= $now then "expired"
           else (.expires_at_epoch_seconds | strftime("%Y-%m-%d %H:%M UTC")) end) ])
    | @tsv
  ' <<<"$body" | column -t -s $'\t'
  info ''
  info 'Revoke one with: sudo ket revoke ID'
}

cmd_revoke() {
  local id=${1:-} yes=false reply status
  [[ ${2:-} == --yes || ${2:-} == -y ]] && yes=true
  [[ $id =~ ^[A-Za-z0-9_-]{1,64}$ ]] || fail 'usage: ket revoke ID [--yes]  (IDs are listed by: sudo ket invites)'
  require_root
  require_installation
  if ! $yes; then
    has_terminal || fail 'no terminal to confirm on; rerun with --yes'
    confirm "Revoke invite $id? Devices using it are disconnected" || { info 'Nothing changed.'; return 0; }
  fi
  reply=$(admin_request DELETE "/v1/admin/access-grants/$id")
  status=${reply##*$'\n'}
  case $status in
    204) info "Invite $id is revoked." ;;
    404) fail "no invite with ID $id" ;;
    *) fail "the server refused to revoke the invite (HTTP $status)" ;;
  esac
}
