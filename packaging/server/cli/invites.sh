# shellcheck shell=bash
# Invites and access grants: create (with an interactive wizard), list, show,
# pause and resume, edit, and revoke. Sourced by the ket command; never
# executed.
# shellcheck disable=SC2034,SC2154

# admin_request METHOD PATH [JSON]: calls the loopback admin API and prints
# the body followed by a final line holding the HTTP status.
admin_request() {
  local token data=()
  token=$(env_value KET_ADMIN_TOKEN)
  [[ -n $token ]] || fail 'KET_ADMIN_TOKEN is missing from .env'
  (($# >= 3)) && data=(--header 'Content-Type: application/json' --data-binary "$3")
  curl --silent --show-error --max-time 30 --request "$1" \
    --header "Authorization: Bearer $token" \
    "${data[@]}" \
    --write-out '\n%{http_code}' \
    "$(control_url "$2")" \
    || fail 'could not reach Ket on this server; check: sudo ket status'
}

# admin_call EXPECTED METHOD PATH [JSON]: prints the reply body when the
# status is EXPECTED; otherwise fails with the server's reason.
admin_call() {
  local expected=$1 reply status body reason
  shift
  reply=$(admin_request "$@")
  status=${reply##*$'\n'}
  body=${reply%$'\n'*}
  [[ $status == "$expected" ]] && { printf '%s' "$body"; return 0; }
  [[ $status == 404 ]] && fail "no invite with that ID (list them with: sudo ket invites)"
  reason=$(jq -r '.message // empty' <<<"$body" 2>/dev/null | tr -d '[:cntrl:]' | cut -c1-200)
  fail "the server refused (HTTP $status${reason:+: $reason})"
}

offered_protocols() { admin_call 200 GET /v1/admin/protocols; }

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

# duration_seconds TEXT: 7d, 12h, 30m, or a bare number of days.
duration_seconds() {
  [[ $1 =~ ^([1-9][0-9]{0,5})([dhm]?)$ ]] || return 1
  local amount=${BASH_REMATCH[1]}
  case ${BASH_REMATCH[2]} in
    h) printf '%s' $((amount * 3600)) ;;
    m) printf '%s' $((amount * 60)) ;;
    *) printf '%s' $((amount * 86400)) ;;
  esac
}

# new_end CHANGE CURRENT NOW: the end time after CHANGE (+7d extends from the
# current end, or from now once it has passed; -3d shortens; 30d means 30
# days from now).
new_end() {
  local change=$1 current=$2 now=$3 seconds base
  case $change in
    +*) seconds=$(duration_seconds "${change#+}") || return 1
        base=$((current > now ? current : now))
        printf '%s' $((base + seconds)) ;;
    -*) seconds=$(duration_seconds "${change#-}") || return 1
        printf '%s' $((current - seconds)) ;;
    *) seconds=$(duration_seconds "$change") || return 1
       printf '%s' $((now + seconds)) ;;
  esac
}

utc_time() { date -u -d "@$1" '+%Y-%m-%d %H:%M UTC' 2>/dev/null || printf 'Unix time %s' "$1"; }

invite_usage() {
  cat <<'USAGE'
Usage: ket invite [--label TEXT] [--devices N] [--days N | --minutes N]
                  [--protocols LIST] [--data SIZE/PERIOD] [--no-qr]

Without options on a terminal, ket asks who the invite is for, how many
devices may be connected at once, for how many days, which protocols it may
use, and how much data its devices may use together. LIST is "all" (the
default) or names such as "stealth reality"; "sudo ket invites protocols"
lists the names this server offers. --data takes a size and a period such as
50GB/month, 2GB/day, or 10GB/week (UTC; weeks start on Monday); without it
there is no limit.
USAGE
}

cmd_invite() {
  case ${1:-} in -h|--help) invite_usage; return 0 ;; esac
  require_root
  require_installation
  if (($# == 0)) && has_terminal; then
    local label devices days protocols data
    info 'Create an invite. Press Enter to accept the suggestion in brackets.'
    label=$(ask 'Who is it for (a name you will recognise later)' 'Ket devices')
    devices=$(ask_number 'How many devices may be connected at the same time' 1 512) || return 1
    days=$(ask_number 'For how many days should it work' 30 365) || return 1
    protocols=$(ask_protocols all) || return 1
    data=$(ask_allowance none) || return 1
    set -- --label "$label" --devices "$devices" --days "$days" --protocols "$protocols"
    [[ ${data,,} == none ]] || set -- "$@" --data "$data"
  fi
  (cd "$install_dir" && ./packaging/server/create-invite.sh "$@")
}

# ask_protocols DEFAULT: asks which protocols to allow until the answer is
# "all" or names this server offers.
ask_protocols() {
  local offered answer attempt
  offered=$(offered_protocols) || return 1
  info "This server offers: $(jq -r 'map("\(.name) (\(.display_name))") | join(", ")' <<<"$offered")" >> "$(ket_tty)"
  for attempt in 1 2 3; do
    answer=$(ask 'Which protocols may it use ("all", or names)' "$1")
    if protocol_access_json "$answer" "$offered" >/dev/null 2>> "$(ket_tty)"; then
      printf '%s' "$answer"
      return 0
    fi
  done
  fail 'no valid answer given'
}

# ask_allowance DEFAULT: asks how much data the invite's devices may use
# together until the answer is a size and a period, or "none".
ask_allowance() {
  local answer attempt
  for attempt in 1 2 3; do
    answer=$(ask 'How much data may its devices use together (such as 50GB/month, or none)' "$1")
    if allowance_json "$answer" >/dev/null 2>> "$(ket_tty)"; then
      printf '%s' "$answer"
      return 0
    fi
  done
  fail 'no valid answer given'
}

invites_usage() {
  cat <<'USAGE'
Usage: sudo ket invites [COMMAND]

  (nothing)            List invites
  show ID              Details and the devices connected through it
  pause ID             Stop it working for now; its devices disconnect
  resume ID            Let a paused invite work again
  edit ID [options]    Change it (asks, on a terminal, without options):
      --label TEXT       Rename it
      --devices N        Devices that may be connected at once
      --extend 7d        End later (d = days, h = hours, m = minutes)
      --shorten 3d       End sooner
      --ends-in 30d      End this long from now
      --protocols LIST   "all", or names such as "stealth reality"
      --data 50GB/month  Data its devices may use together (or none);
                         also /day or /week, counted in UTC
      --reset-data       Start this period's data count again
  revoke ID [--yes]    Delete it for good and disconnect its devices
  protocols            The protocol names this server offers

Each change takes effect at once. Devices disconnected by a change connect
again with the new settings, or are refused if it no longer lets them in.
When an invite's devices use up its data allowance they are disconnected
and refused until the period starts again, the allowance is raised, or the
count is reset.
USAGE
}

cmd_invites() {
  local command=${1:-list}
  (($#)) && shift
  case $command in
    list) invites_list "$@" ;;
    show) invites_show "$@" ;;
    pause) invites_set_paused true "$@" ;;
    resume) invites_set_paused false "$@" ;;
    edit) invites_edit "$@" ;;
    revoke) cmd_revoke "$@" ;;
    protocols) invites_protocols "$@" ;;
    -h|--help|help) invites_usage ;;
    *) invites_usage >&2; fail "unknown invites command: $command" ;;
  esac
}

invite_preamble() {
  require_root
  require_installation
  require_commands curl jq
}

# require_invite_id ID: fails unless ID looks like an invite ID.
require_invite_id() {
  [[ ${1:-} =~ ^[A-Za-z0-9_-]{1,64}$ ]] || fail 'give the invite ID (list them with: sudo ket invites)'
}

invites_list() {
  invite_preamble
  local body offered
  body=$(admin_call 200 GET /v1/admin/access-grants) || return 1
  if [[ $(jq 'length' <<<"$body") == 0 ]]; then
    info 'No invites yet. Create one with: sudo ket invite'
    return 0
  fi
  offered=$(offered_protocols) || return 1
  jq -r --argjson now "$(date +%s)" --argjson offered "$offered" "$ALLOWANCE_JQ"'
    def names: if . == "all" or . == null then "all"
      else .only | map(. as $p | ($offered | map(select(.protocol == $p)) | first | .name) // $p)
        | join(",") end;
    ["ID", "FOR", "STATUS", "IN USE", "DATA", "ENDS", "PROTOCOLS"],
    (sort_by(.created_at_epoch_seconds) | reverse | .[]
      | [ .id,
          (.label | gsub("[[:cntrl:]]"; "") | .[0:32]),
          (if .expires_at_epoch_seconds != null and .expires_at_epoch_seconds <= $now then "expired"
           elif .paused then "paused" else "active" end),
          "\(.active_connections)/\(.max_connections)",
          (.allowance | allowance_short),
          (if .expires_at_epoch_seconds == null then "never"
           else (.expires_at_epoch_seconds | strftime("%Y-%m-%d %H:%M UTC")) end),
          (.protocols | names) ])
    | @tsv
  ' <<<"$body" | column -t -s $'\t'
  info ''
  info 'Details: sudo ket invites show ID    More: sudo ket invites --help'
}

invites_show() {
  require_invite_id "${1:-}"
  invite_preamble
  local body offered now
  body=$(admin_call 200 GET "/v1/admin/access-grants/$1") || return 1
  offered=$(offered_protocols) || return 1
  now=$(date +%s)
  jq -r --argjson now "$now" '
    def when: strftime("%Y-%m-%d %H:%M UTC");
    def count($unit): "\(.) \($unit)\(if . == 1 then "" else "s" end)";
    def left: (. - $now) as $s
      | if $s <= 0 then "ended"
        elif $s >= 86400 then "in \($s / 86400 | floor | count("day"))"
        elif $s >= 3600 then "in \($s / 3600 | floor | count("hour"))"
        else "in \([($s / 60 | floor), 1] | max | count("minute"))" end;
    "Invite \(.id): \(.label | gsub("[[:cntrl:]]"; ""))",
    "  Status:     \(if .expires_at_epoch_seconds != null and .expires_at_epoch_seconds <= $now then "expired"
                      elif .paused then "paused (resume with: sudo ket invites resume \(.id))"
                      else "active" end)",
    "  Ends:       \(if .expires_at_epoch_seconds == null then "never"
                      else "\(.expires_at_epoch_seconds | when) (\(.expires_at_epoch_seconds | left))" end)",
    "  Devices:    \(.active_connections) of \(.max_connections) connected",
    "  Created:    \(.created_at_epoch_seconds | when)"
  ' <<<"$body"
  info "  Data:       $(jq -r "$ALLOWANCE_JQ"' .allowance | allowance_line' <<<"$body")"
  if jq -e '.allowance != null and .allowance.used_bytes >= .allowance.bytes' <<<"$body" >/dev/null; then
    info "              Used up: its devices are refused until then, or: sudo ket invites edit $1 --reset-data"
  fi
  info "  Protocols:  $(protocol_names "$(jq -c '.protocols' <<<"$body")" "$offered")"
  info ''
  if [[ $(jq '.devices | length' <<<"$body") == 0 ]]; then
    info 'No device is connected through it right now.'
  else
    jq -r "$DEVICE_JQ $ALLOWANCE_JQ"'
      ["DEVICE", "CONNECTED SINCE", "TRAFFIC"],
      (.devices[] | [ device,
                      (.connected_at_epoch_seconds | strftime("%Y-%m-%d %H:%M UTC")),
                      (if .traffic.available then (.traffic.bytes_sent + .traffic.bytes_received | size)
                       else "unknown" end) ])
      | @tsv
    ' <<<"$body" | column -t -s $'\t'
  fi
  invite_activity "$body" "$offered"
}

# The app's name and the start of its session ID, which tells two devices
# with the same app apart and matches them up across the tables.
DEVICE_JQ='def device: (.client_name | gsub("[[:cntrl:]]"; "") | .[0:32])
  + (if .session_id then " #\(.session_id[0:4])" else "" end);'

# What each activity event means; every name in
# crates/ket-core/tests/vectors/grant-activity-events.json has an entry.
# shellcheck disable=SC2016 # jq, not shell, expands these.
ACTIVITY_JQ='def protocol: . as $p | ($offered | map(select(.protocol == $p)) | first | .display_name) // $p;
  def what: {
    enrolled: "Connected",
    checked_in: "Checked in",
    left: "Disconnected",
    lease_ended: "Stopped checking in, so its session ended",
    ended_by_change: "Disconnected by a change to the invite",
    ended_allowance_used: "Disconnected: the invite used up its data allowance",
    refused_wrong_code: "Refused: wrong access code",
    refused_expired: "Refused: the invite has ended",
    refused_paused: "Refused: the invite is paused",
    refused_allowance_used: "Refused: the data allowance is used up",
    refused_device_limit: "Refused: the device limit is reached",
    refused_server_full: "Refused: the server is full",
    refused_update_required: "Refused: the app must be updated",
    refused_no_protocol: "Refused: the server offers no protocol this invite allows",
    refused_server_unavailable: "Refused: the protocol servers were not ready",
    signed_in: "Reached the server over \(.protocol | protocol)",
    sign_in_refused: "\(.protocol | protocol) refused it: its session had ended",
    sign_in_refused_protocol: "\(.protocol | protocol) refused it: the invite does not allow it"
  }[.event] // .event;'

# activity_what EVENT_JSON OFFERED: one event in words.
activity_what() { jq -r --argjson offered "$2" "$ACTIVITY_JQ"' what' <<<"$1"; }

# invite_activity DETAILS OFFERED: what devices recently did with the invite,
# newest first. Older servers keep no activity, so nothing is printed.
invite_activity() {
  [[ $(jq '.recent // [] | length' <<<"$1") != 0 ]] || return 0
  info ''
  info 'Recent activity (since Ket last started; kept in memory only, without addresses):'
  jq -r --argjson now "$(date +%s)" --argjson offered "$2" "$DEVICE_JQ $ACTIVITY_JQ"'
    def ago: ($now - .) as $s
      | if $s < 60 then "just now" elif $s < 3600 then "\($s / 60 | floor) min ago"
        elif $s < 86400 then "\($s / 3600 | floor) h ago" else "\($s / 86400 | floor) d ago" end;
    ["WHEN", "DEVICE", "WHAT", "TIMES"],
    (.recent[] | [ (.last_at_epoch_seconds | ago), device, what, "\(.count)" ])
    | @tsv
  ' <<<"$1" | column -t -s $'\t'
  info ''
  info 'OpenVPN and Hysteria 2 report when a device reaches them. A device that connects'
  info 'but never reaches them is probably on a network that blocks those protocols.'
}

# report_change REPLY: says what a change did to connected devices.
report_change() {
  local disconnected
  disconnected=$(jq -r '.disconnected_devices' <<<"$1")
  ((disconnected == 0)) || info "$disconnected connected device(s) were disconnected."
}

invites_set_paused() {
  local paused=$1 id=${2:-} reply
  require_invite_id "$id"
  invite_preamble
  reply=$(admin_call 200 PATCH "/v1/admin/access-grants/$id" "{\"paused\":$paused}") || return 1
  if [[ $paused == true ]]; then
    info "Invite $id is paused. Its devices cannot connect until: sudo ket invites resume $id"
  else
    info "Invite $id works again. Its devices can connect."
  fi
  report_change "$reply"
}

invites_edit() {
  local id=${1:-}
  require_invite_id "$id"
  shift
  invite_preamble
  local current now label='' devices='' change='' protocols='' data='' reset=false changes end offered
  current=$(admin_call 200 GET "/v1/admin/access-grants/$id") || return 1
  now=$(date +%s)
  while (($#)); do
    case $1 in
      --label) [[ $# -ge 2 ]] || fail '--label needs a value'; label=$2; shift 2 ;;
      --devices) [[ $# -ge 2 ]] || fail '--devices needs a value'; devices=$2; shift 2 ;;
      --extend) [[ $# -ge 2 ]] || fail '--extend needs a duration such as 7d'; change="+$2"; shift 2 ;;
      --shorten) [[ $# -ge 2 ]] || fail '--shorten needs a duration such as 3d'; change="-$2"; shift 2 ;;
      --ends-in) [[ $# -ge 2 ]] || fail '--ends-in needs a duration such as 30d'; change=$2; shift 2 ;;
      --protocols) [[ $# -ge 2 ]] || fail '--protocols needs "all" or names'; protocols=$2; shift 2 ;;
      --data) [[ $# -ge 2 ]] || fail '--data needs a size and period such as 50GB/month, or none'; data=$2; shift 2 ;;
      --reset-data) reset=true; shift ;;
      -h|--help) invites_usage; return 0 ;;
      *) fail "unknown option: $1 (see: sudo ket invites --help)" ;;
    esac
  done
  offered=$(offered_protocols) || return 1
  if [[ -z $label$devices$change$protocols$data ]] && ! $reset; then
    has_terminal || fail 'nothing to change; see: sudo ket invites --help'
    {
      invites_show "$id"
      info ''
      info 'Press Enter to keep what is in brackets.'
    } >> "$(ket_tty)"
    label=$(ask 'Name' "$(jq -r '.label' <<<"$current")")
    devices=$(ask_number 'Devices connected at the same time' "$(jq -r '.max_connections' <<<"$current")" 512) || return 1
    change=$(ask 'When it ends: +7d extends, -3d shortens, 30d is 30 days from now' 'keep')
    [[ $change == keep ]] && change=''
    protocols=$(ask_protocols "$(protocol_names "$(jq -c '.protocols' <<<"$current")" "$offered" | tr -d ' ')") || return 1
    data=$(ask_allowance "$(current_allowance_text "$current")") || return 1
  fi

  changes='{}'
  if [[ -n $label && $label != "$(jq -r '.label' <<<"$current")" ]]; then
    changes=$(jq -c --arg label "$label" '.label = $label' <<<"$changes")
  fi
  if [[ -n $devices && $devices != "$(jq -r '.max_connections' <<<"$current")" ]]; then
    [[ $devices =~ ^[1-9][0-9]{0,5}$ ]] || fail '--devices must be a whole number from 1'
    changes=$(jq -c --argjson devices "$devices" '.max_connections = $devices' <<<"$changes")
  fi
  if [[ -n $change ]]; then
    end=$(new_end "$change" "$(jq -r '.expires_at_epoch_seconds // 0' <<<"$current")" "$now") \
      || fail "\"${change#[+-]}\" is not a duration; use a number with d, h, or m, such as 7d"
    ((end > now)) || fail "that would end it in the past; to stop it now use: sudo ket invites pause $id"
    changes=$(jq -c --argjson end "$end" '.expires_at_epoch_seconds = $end' <<<"$changes")
  fi
  if [[ -n $protocols ]]; then
    local access
    access=$(protocol_access_json "$protocols" "$offered") || fail 'no change made'
    if [[ $access != "$(jq -c '.protocols' <<<"$current")" ]]; then
      changes=$(jq -c --argjson access "$access" '.protocols = $access' <<<"$changes")
    fi
  fi
  if [[ -n $data ]]; then
    local allowance
    allowance=$(allowance_json "$data") || fail 'no change made'
    if [[ $allowance != "$(jq -c '.allowance // null | if . == null then null else {bytes, period} end' <<<"$current")" ]]; then
      changes=$(jq -c --argjson allowance "$allowance" '.allowance = $allowance' <<<"$changes")
    fi
  fi
  $reset && changes=$(jq -c '.reset_usage = true' <<<"$changes")
  if [[ $changes == '{}' ]]; then
    info 'Nothing changed.'
    return 0
  fi

  local reply
  reply=$(admin_call 200 PATCH "/v1/admin/access-grants/$id" "$changes") || return 1
  info "Invite $id is updated."
  if jq -e 'has("expires_at_epoch_seconds")' <<<"$changes" >/dev/null; then
    info "It now ends $(utc_time "$(jq -r '.grant.expires_at_epoch_seconds' <<<"$reply")")."
    info 'Devices that use it see the new date the next time they reach the server.'
    if (( $(jq -r '.expires_at_epoch_seconds // 0' <<<"$current") <= now )); then
      warn 'it had already ended, so apps had forgotten it; share a new invite with: sudo ket invite'
    fi
  fi
  if jq -e 'has("allowance") or has("reset_usage")' <<<"$changes" >/dev/null; then
    info "Data: $(jq -r "$ALLOWANCE_JQ"' .grant.allowance | allowance_line' <<<"$reply")"
  fi
  report_change "$reply"
}

# current_allowance_text DETAILS: the invite's allowance as --data takes it,
# such as 50GB/month, or none.
current_allowance_text() {
  jq -r '.allowance // null | if . == null then "none" else
    (if .bytes % 1099511627776 == 0 then "\(.bytes / 1099511627776)TB"
     elif .bytes % 1073741824 == 0 then "\(.bytes / 1073741824)GB"
     else "\(.bytes / 1048576 | floor)MB" end) + "/\(.period)" end' <<<"$1"
}

invites_protocols() {
  invite_preamble
  info 'Protocols this server offers (use these names with --protocols):'
  offered_protocols | jq -r '.[] | "  \(.name)\t\(.display_name)"' | column -t -s $'\t'
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
