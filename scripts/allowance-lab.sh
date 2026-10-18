#!/usr/bin/env bash
# Allowance lab: checks on the canary that a data allowance counts and stops
# every protocol, with the shipped client code in a disposable Ubuntu 24.04
# desktop-like container.
#
#   scripts/allowance-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...
#                            [--cache DIR] [--client FILE] [--deb FILE]
#
# For each protocol it creates a one-device invite with 1 MB a day, and:
#   1. connects and downloads through the tunnel until `ket invites show`
#      says the allowance is used up,
#   2. checks the server disconnected the device and logged why, and that
#      the tunnel stopped carrying traffic,
#   3. checks a new device is refused with grant_allowance_used,
#   4. resets the count with `ket invites edit ID --reset-data` and checks a
#      device enrolls again,
#   5. revokes the invite.
# Protocols default to every one the canary offers. Needs Docker with
# privileged containers.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# shellcheck source=scripts/lib/desktop-lab.sh
. "$ROOT/scripts/lib/desktop-lab.sh"

readonly NETWORK=ket-allowance-lab
readonly PROBE_URL=https://api.ipify.org
readonly PUSH_URL='https://speed.cloudflare.com/__down?bytes=2000000'
readonly ALL_PROTOCOLS=(vless_xtls_reality stealth shadowsocks2022 xor_scrambled wire_guard open_vpn_stunnel)

failures=0
container=''
client_pid=''
ids=()

step() { printf '\n== %s\n' "$1"; }
pass() { printf '  ok    %s\n' "$1"; }
note() { printf '  note  %s\n' "$1"; }
problem() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
die() {
  printf 'allowance-lab: %s\n' "$1" >&2
  exit 2
}

# invite_id OUTPUT: the invite's id from `ket invite` output, which is the
# start of its access code (the code itself is never printed).
invite_id() {
  local code
  code=$(sed -n 's/.*access code \([^[:space:]]*\)$/\1/p' <<<"$1" | head -n 1)
  [[ ${#code} -gt 10 ]] && printf '%s\n' "${code:0:10}"
}

show() { canary sudo -n ket invites show "$1" 2>&1; }

# wait_for ID PATTERN SECONDS: true once `ket invites show ID` matches.
wait_for() {
  local _
  for _ in $(seq 1 "$3"); do
    show "$1" | grep -Eq "$2" && return 0
    sleep 1
  done
  return 1
}

# enroll INVITE: runs the shipped enrollment against the canary; prints its
# output and fails like it.
enroll() {
  (cd "$ROOT" && cargo run -q -p ket-client-core --example invite_enroll_check <<<"$1" 2>&1)
}

# wait_client_error LOG SECONDS: true once the client reported phase=Error.
wait_client_error() {
  local _
  for _ in $(seq 1 "$2"); do
    grep -q '^phase=Error' "$1" && return 0
    sleep 1
  done
  return 1
}

# run_protocol PROTOCOL
run_protocol() {
  local protocol=$1 label output invite id client_log egress pushed=0 _ result
  step "$protocol"
  label="allowance lab $protocol"
  output=$(canary sudo -n ket invite --label "$(printf '%q' "$label")" --devices 1 --minutes 30 \
    --data 1MB/day --no-qr 2>&1) || { problem "ket invite --data failed: $output"; return; }
  invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || { problem 'the canary printed no invite'; return; }
  id=$(invite_id "$output") || { problem 'the canary printed no access code'; return; }
  ids+=("$id")
  pass "invite $id with $(grep -m 1 'Data' <<<"$output" | sed 's/^[[:space:]]*//')"

  start_desktop "ket-allowance-lab-$$" "$NETWORK" "$cache/engines" "$probe"
  client_log=$(mktemp)
  start_client ket-allowance-client "$client_log" "$protocol" "$invite" --maintain
  if ! wait_connected "$client_log" "$client_pid" 90; then
    if grep -q 'is not available for this invite' "$client_log"; then
      note "the canary does not offer $protocol; skipped"
    else
      problem "could not connect with $protocol within 90 s: $(tr '\n' ' ' <"$client_log")"
    fi
    finish_protocol "$client_log"
    return
  fi
  egress=$(lab curl -4 -s -m 15 "$PROBE_URL" || true)
  if [[ -n $egress ]]; then pass "connected ($(grep -m 1 '^connected' "$client_log"))"; else
    problem 'the site does not load through the tunnel'
  fi

  # 1. Use the allowance up.
  for _ in $(seq 1 15); do
    if lab curl -4 -s -m 20 -o /dev/null "$PUSH_URL"; then pushed=$((pushed + 2)); fi
    show "$id" | grep -q 'Used up:' && break
    sleep 2
  done
  if wait_for "$id" 'Used up:' 30; then
    pass "used up after about $pushed MB through the tunnel: $(show "$id" | grep -m 1 'Data:' | sed 's/^[[:space:]]*//')"
  else
    problem "the allowance was never used up ($pushed MB pushed): $(show "$id" | grep -m 1 'Data:')"
    finish_protocol "$client_log"
    return
  fi

  # 2. The device is gone, and why is logged.
  if wait_for "$id" 'Devices: +0 of 1 connected' 30; then pass 'the server disconnected the device'; else
    problem "the device is still connected: $(show "$id" | grep -m 1 'Devices:')"
  fi
  if show "$id" | grep -q 'Disconnected: the invite used up its data allowance'; then
    pass 'the activity log says it used up its data allowance'
  else
    problem 'the activity log does not say why the device was disconnected'
  fi
  sleep 5
  result=$(lab curl -4 -s -m 10 "$PROBE_URL" || true)
  if [[ $result == "$egress" ]]; then
    problem 'the site still loads through the tunnel after the allowance was used up'
  else
    pass 'the tunnel no longer carries traffic'
  fi
  # A protocol that recovers by itself (WireGuard) tries to reconnect first;
  # its renewal is then refused.
  if wait_client_error "$client_log" 60; then pass 'the client stopped with an error'; else
    problem "the client did not stop within 60 s: $(tail -n 3 "$client_log" | tr '\n' ' ')"
  fi
  finish_protocol "$client_log"

  # 3. A new device is refused; 4. after a reset it enrolls.
  if result=$(enroll "$invite"); then
    problem 'a new device enrolled although the allowance is used up'
  elif grep -q 'grant_allowance_used' <<<"$result"; then
    pass 'a new device is refused with grant_allowance_used'
  else
    problem "a new device was refused for another reason: $result"
  fi
  output=$(canary sudo -n ket invites edit "$id" --reset-data 2>&1) || problem "--reset-data failed: $output"
  if result=$(enroll "$invite"); then pass 'after --reset-data a device enrolls again'; else
    problem "after --reset-data enrollment still fails: $result"
  fi
  if canary sudo -n ket revoke "$id" --yes >/dev/null 2>&1; then pass "invite $id revoked"; else
    problem "could not revoke invite $id; revoke it with: sudo ket revoke $id"
  fi
}

# finish_protocol LOG: stops the client and removes the container.
finish_protocol() {
  lab systemctl kill --signal=TERM ket-allowance-client.service 2>/dev/null || true
  wait "$client_pid" 2>/dev/null || true
  rm -f -- "$1"
  docker rm -f "$container" >/dev/null
  container=''
}

cleanup() {
  local id
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  for id in "${ids[@]}"; do
    canary sudo -n ket invites show "$id" >/dev/null 2>&1 || continue
    canary sudo -n ket revoke "$id" --yes >/dev/null 2>&1 \
      || printf 'allowance-lab: could not revoke %s; revoke it with: sudo ket revoke %s\n' "$id" "$id" >&2
  done
}

main() {
  local protocol deb=''
  local -a protocols=()
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab"
  probe=''
  [[ $# -ge 1 && $1 != -* ]] || { sed -n '2,19p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2; }
  canary_target=$1
  canary_ssh_options=()
  shift
  while (($#)); do
    case $1 in
      --ssh-key) [[ $# -ge 2 ]] || die '--ssh-key needs a file'; canary_ssh_options+=(-i "$2" -o IdentitiesOnly=yes); shift 2 ;;
      --protocol) [[ $# -ge 2 ]] || die '--protocol needs a name'; protocols+=("$2"); shift 2 ;;
      --cache) [[ $# -ge 2 ]] || die '--cache needs a directory'; cache=$2; shift 2 ;;
      --client) [[ $# -ge 2 && -f $2 ]] || die '--client needs a connect_check binary'; probe=$(realpath "$2"); shift 2 ;;
      --deb) [[ $# -ge 2 && -f $2 ]] || die '--deb needs a Ket .deb'; deb=$2; shift 2 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  ((${#protocols[@]})) || protocols=("${ALL_PROTOCOLS[@]}")
  for tool in docker cargo ssh jq; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  step 'Preparing the lab'
  fetch_engines "$cache/engines"
  [[ -z $deb ]] || engines_from_deb "$deb" "$cache/engines"
  [[ -n $probe ]] || probe=$(build_probe)
  (cd "$ROOT" && cargo build -q -p ket-client-core --example invite_enroll_check)
  build_image
  docker network inspect "$NETWORK" >/dev/null 2>&1 || docker network create "$NETWORK" >/dev/null
  pass "image $DESKTOP_LAB_IMAGE, engines in $cache/engines"
  trap cleanup EXIT

  for protocol in "${protocols[@]}"; do
    run_protocol "$protocol"
  done

  printf '\n'
  if ((failures > 0)); then
    printf 'ALLOWANCE FAILURES: %d check(s) failed.\n' "$failures"
    exit 1
  fi
  printf 'Every protocol counted the allowance, stopped at it, and came back after a reset.\n'
}

main "$@"
