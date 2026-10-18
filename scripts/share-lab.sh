#!/usr/bin/env bash
# Share lab: shares a disposable Ubuntu 24.04 desktop-like container's Ket
# connection with a second container on the same network, the way the
# desktop app shares with a phone, and checks that the nearby device only
# ever gets out through the tunnel.
#
#   scripts/share-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...
#                        [--cache DIR] [--client FILE] [--share FILE]
#
# The share (the share_check example, running unprivileged like the app)
# starts once, before Ket connects, and stays up while each protocol connects
# and disconnects underneath it. Then it checks that
#   1. with no tunnel the share refuses (503) and nothing leaves the
#      container, before the first connection and after every disconnect;
#   2. over the HTTP proxy and over SOCKS5 a site sees the tunnel's exit, and
#      plain http:// pages load;
#   3. the share sent nothing out of the container's interface but its
#      answers to the device, and no name lookup left it, so nothing for the
#      device leaked;
#   4. the device cannot reach the sharing computer itself (its loopback
#      services or its own address) through the share;
#   5. the port never changes while protocols change;
#   6. a wrong password is refused (407), and after five wrong tries even the
#      right one is turned away;
#   7. the share ends on its own when its network goes away.
# Protocols default to vless_xtls_reality. Needs Docker with privileged
# containers; the invite is revoked afterwards. The share's password stays
# in the lab: it is passed to curl on stdin and never printed.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# shellcheck source=scripts/lib/desktop-lab.sh
. "$ROOT/scripts/lib/desktop-lab.sh"

readonly NETWORK=ket-share-lab
readonly TRACE_URL=https://www.cloudflare.com/cdn-cgi/trace
readonly PLAIN_URL=http://example.com/
readonly NOBODY=65534
readonly CREDENTIALS=/run/ket-share/credentials

failures=0
container=''
device=''
client_pid=''
share_pid=''
share_log=''
label=''
address=''
device_address=''
port=''
password=''
captures=0
pcap=''

step() { printf '\n== %s\n' "$1"; }
pass() { printf '  ok    %s\n' "$1"; }
note() { printf '  note  %s\n' "$1"; }
problem() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
die() {
  printf 'share-lab: %s\n' "$1" >&2
  exit 2
}

# through_share PASSWORD CURL_ARGUMENT...: curl on the nearby device through
# the share's HTTP proxy, signed in with PASSWORD given on stdin.
through_share() {
  local secret=$1
  shift
  printf 'proxy-user = "ket:%s"\n' "$secret" \
    | docker exec -i "$device" curl -K - -s -m 25 -x "http://$address:$port" "$@" || true
}

# through_socks CURL_ARGUMENT...: the same through the share's SOCKS5 side.
through_socks() {
  printf 'proxy-user = "ket:%s"\n' "$password" \
    | docker exec -i "$device" curl -K - -s -m 25 --socks5-hostname "$address:$port" "$@" || true
}

exit_address() { sed -n 's/^ip=//p'; }

# Leaks are counted two ways while the device uses the share:
#   - netfilter counts every packet the share's own user sends out of the
#     computer's network interface to anywhere but the nearby device; the
#     share's connections and lookups all leave through the tunnel, so the
#     count must stay 0;
#   - tcpdump records every name lookup the computer sends out of that
#     interface, from any process, which must be none.
# Each capture gets a new file (tcpdump owns it once it drops privileges)
# and must be seen running, so one that never started cannot pass for clean.
start_leak_counter() {
  local tables
  for tables in iptables ip6tables; do
    lab "$tables" -N KET_SHARE_LEAK
    lab "$tables" -A KET_SHARE_LEAK -j RETURN
  done
  lab iptables -I OUTPUT -o eth0 -m owner --uid-owner "$NOBODY" ! -d "$device_address" -j KET_SHARE_LEAK
  lab ip6tables -I OUTPUT -o eth0 -m owner --uid-owner "$NOBODY" -j KET_SHARE_LEAK
}

# leak_count: packets counted since the last watch_leaks.
leak_count() {
  local tables total=0 packets
  for tables in iptables ip6tables; do
    packets=$(lab "$tables" -L KET_SHARE_LEAK -v -x -n | awk '$3 == "RETURN" { print $1; exit }')
    total=$((total + ${packets:-0}))
  done
  printf '%s\n' "$total"
}

watch_leaks() {
  lab iptables -Z KET_SHARE_LEAK
  lab ip6tables -Z KET_SHARE_LEAK
  captures=$((captures + 1))
  pcap=/tmp/share-$captures.pcap
  lab sh -c "tcpdump -i eth0 -Q out -n --immediate-mode -U -w $pcap 'port 53 or port 853 or port 5355' >/dev/null 2>&1 &"
  sleep 2
  lab pgrep -x tcpdump >/dev/null || die "the lookup capture did not start ($pcap)"
}

check_leaks() {
  local what=$1 count lookups
  # Packets of the last request may still be on their way to the capture.
  sleep 1
  lab pkill -INT tcpdump || true
  sleep 1
  count=$(leak_count)
  lookups=$(lab sh -c "tcpdump -n -r $pcap 2>/dev/null | head -n 3")
  if [[ $count == 0 && -z $lookups ]]; then
    pass "$what"
  elif [[ $count != 0 ]]; then
    problem "the share sent $count packet(s) outside the tunnel"
  else
    problem "name lookups left outside the tunnel: $(tr '\n' ';' <<<"$lookups")"
  fi
}

# leak_counter_works: the share's user reaching the network directly is
# counted, so a count of 0 means nothing left rather than nothing counted.
leak_counter_works() {
  local count
  watch_leaks
  docker exec -u "$NOBODY:$NOBODY" "$container" curl -4 -s -m 10 -o /dev/null "$PLAIN_URL" || true
  lab pkill -INT tcpdump || true
  count=$(leak_count)
  if ((count > 0)); then
    pass "the leak counter counts the share's user going out directly ($count packets)"
  else
    problem 'the leak counter counted nothing, so it cannot show a leak'
  fi
}

# refused_without_tunnel WHEN: with no tunnel the device is told so and
# nothing leaves.
refused_without_tunnel() {
  local code
  watch_leaks
  code=$(through_share "$password" -o /dev/null -w '%{http_code}' "$PLAIN_URL")
  if [[ $code == 503 ]]; then
    pass "$1 the share answers 503 (Ket is not connected)"
  else
    problem "$1 the share answered ${code:-nothing} instead of 503"
  fi
  check_leaks "$1 nothing a nearby device sent left the computer"
}

# run_protocol PROTOCOL INVITE FIRST
run_protocol() {
  local protocol=$1 invite=$2 first=$3 client_log direct seen code
  step "$protocol"
  client_log=$(mktemp)
  start_client ket-share-client "$client_log" "$protocol" "$invite"
  if ! wait_connected "$client_log" "$client_pid" 90; then
    if grep -q 'is not available for this invite' "$client_log"; then
      note "the canary does not offer $protocol; skipped"
    else
      problem "could not connect with $protocol within 90 s: $(tr '\n' ' ' <"$client_log")"
    fi
    finish_protocol "$client_log" skipped
    return
  fi
  watch_leaks

  direct=$(docker exec "$device" curl -4 -s -m 15 "$TRACE_URL" | exit_address || true)
  seen=$(through_share "$password" "$TRACE_URL" | exit_address)
  if [[ -n $seen && $seen != "$direct" ]]; then
    pass 'over the HTTP proxy a site sees the tunnel exit, not the network'
  else
    problem "over the HTTP proxy a site saw ${seen:-nothing} (the network's own address is ${direct:-unknown}; $(through_share "$password" -S -o /dev/null "$TRACE_URL" 2>&1 | tr '\n' ' '))"
  fi
  seen=$(through_socks "$TRACE_URL" | exit_address)
  if [[ -n $seen && $seen != "$direct" ]]; then
    pass 'over SOCKS5 a site sees the tunnel exit'
  else
    problem "over SOCKS5 a site saw ${seen:-nothing} ($(through_socks -S -o /dev/null "$TRACE_URL" 2>&1 | tr '\n' ' '))"
  fi
  if through_share "$password" "$PLAIN_URL" | grep -q 'Example Domain'; then
    pass 'plain http:// pages load through the share'
  else
    problem "a plain http:// page did not load through the share ($(through_share "$password" -S -o /dev/null -w 'status %{http_code}' "$PLAIN_URL" 2>&1 | tr '\n' ' '))"
  fi
  code=$(through_share "$password" -o /dev/null -w '%{http_code}' "http://127.0.0.1:39731/")
  [[ $code == 403 ]] || problem "the computer's loopback through the share answered ${code:-nothing}, not 403"
  code=$(through_share "$password" -o /dev/null -w '%{http_code}' "http://$address:22/")
  if [[ $code == 403 ]]; then
    pass "the computer itself is not reachable through the share"
  else
    problem "the computer's own address through the share answered ${code:-nothing}, not 403"
  fi
  if $first; then
    code=$(through_share wrongwrongwr -o /dev/null -w '%{http_code}' "$PLAIN_URL")
    if [[ $code == 407 ]]; then pass 'a wrong password is refused (407)'; else
      problem "a wrong password got ${code:-nothing}, not 407"
    fi
    sleep 3
    if grep -q 'refused=1' "$share_log"; then pass 'the refused sign-in is counted'; else
      problem 'the refused sign-in was not counted'
    fi
  fi
  grep -q 'tunnel=true' "$share_log" || problem 'the share never saw the tunnel'
  check_leaks 'nothing for the nearby device left outside the tunnel, DNS included'
  finish_protocol "$client_log"
  refused_without_tunnel 'after disconnecting,'
  if kill -0 "$share_pid" 2>/dev/null && grep -q "^sharing port=$port$" "$share_log"; then
    pass "the share kept port $port"
  else
    problem 'the share did not survive the protocol change'
  fi
}

# finish_protocol LOG [skipped]: disconnects the client.
finish_protocol() {
  local client_log=$1
  lab systemctl kill --signal=TERM ket-share-client.service 2>/dev/null || true
  wait "$client_pid" 2>/dev/null || true
  if [[ ${2:-} != skipped ]] && ! grep -q '^disconnected' "$client_log"; then
    problem "disconnect failed: $(tr '\n' ' ' <"$client_log")"
  fi
  rm -f -- "$client_log"
}

# guessing_is_stopped: five wrong passwords, then even the right one is
# turned away for a minute.
guessing_is_stopped() {
  local attempt code
  step 'Guessing the password'
  for attempt in 1 2 3 4 5; do
    through_share "wrongguess$attempt" -o /dev/null "$PLAIN_URL" >/dev/null
  done
  sleep 1
  code=$(through_share "$password" -o /dev/null -w '%{http_code}' "$PLAIN_URL")
  if [[ $code == 000 || -z $code ]]; then
    pass 'after five wrong passwords the device is turned away, right password or not'
  else
    problem "after five wrong passwords the device still got an answer ($code)"
  fi
}

# network_goes_away: the share stops on its own when its network does.
network_goes_away() {
  local attempt
  step 'Leaving the network'
  docker network disconnect "$NETWORK" "$container" >/dev/null
  for attempt in $(seq 1 10); do
    grep -q '^ended network_gone' "$share_log" && break
    sleep 1
  done
  if grep -q '^ended network_gone' "$share_log"; then
    pass "the share stopped on its own within $attempt s"
  else
    problem 'the share kept running after its network went away'
  fi
}

cleanup() {
  [[ -z $share_pid ]] || kill "$share_pid" 2>/dev/null || true
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  [[ -z $device ]] || docker rm -f "$device" >/dev/null 2>&1 || true
  [[ -z $share_log ]] || rm -f -- "$share_log"
  if [[ -n $label ]] && ! canary_revoke "$label" >/dev/null; then
    printf 'share-lab: could not revoke the invite "%s"; revoke it with: sudo ket invites\n' "$label" >&2
  fi
}

# build_share: builds this tree's share_check and prints its path.
build_share() {
  (cd "$ROOT" && cargo build -q -p ket-client-core --example share_check)
  printf '%s/debug/examples/share_check\n' \
    "$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
}

main() {
  local protocol output invite share='' first=true attempt user
  local -a protocols=()
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab"
  probe=''
  [[ $# -ge 1 && $1 != -* ]] || { sed -n '2,28p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2; }
  canary_target=$1
  canary_ssh_options=()
  shift
  while (($#)); do
    case $1 in
      --ssh-key) [[ $# -ge 2 ]] || die '--ssh-key needs a file'; canary_ssh_options+=(-i "$2" -o IdentitiesOnly=yes); shift 2 ;;
      --protocol) [[ $# -ge 2 ]] || die '--protocol needs a name'; protocols+=("$2"); shift 2 ;;
      --cache) [[ $# -ge 2 ]] || die '--cache needs a directory'; cache=$2; shift 2 ;;
      --client) [[ $# -ge 2 && -f $2 ]] || die '--client needs a connect_check binary'; probe=$(realpath "$2"); shift 2 ;;
      --share) [[ $# -ge 2 && -f $2 ]] || die '--share needs a share_check binary'; share=$(realpath "$2"); shift 2 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  ((${#protocols[@]})) || protocols=(vless_xtls_reality)
  for tool in docker cargo ssh jq; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  step 'Preparing the lab'
  fetch_engines "$cache/engines"
  [[ -n $probe ]] || probe=$(build_probe)
  [[ -n $share ]] || share=$(build_share)
  build_image
  docker network inspect "$NETWORK" >/dev/null 2>&1 || docker network create "$NETWORK" >/dev/null
  trap cleanup EXIT
  start_desktop "ket-share-lab-$$" "$NETWORK" "$cache/engines" "$probe" -v "$share:/opt/ket/share_check:ro"
  device="ket-share-device-$$"
  docker run -d --name "$device" --network "$NETWORK" --entrypoint sleep "$DESKTOP_LAB_IMAGE" infinity >/dev/null
  address=$(lab ip -o -4 addr show eth0 | awk '{ sub("/.*", "", $4); print $4; exit }')
  device_address=$(docker exec "$device" ip -o -4 addr show eth0 | awk '{ sub("/.*", "", $4); print $4; exit }')
  pass "desktop $address and nearby device $device_address on $NETWORK"

  label="share lab $(git -C "$ROOT" rev-parse --short HEAD) $$"
  output=$(canary_invite "$label" 1) || die "ket invite failed on the canary: $output"
  invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || die 'the canary printed no invite'
  pass "invite from $canary_target ($label)"

  step 'Starting the share, unprivileged'
  lab install -d -m 0700 -o "$NOBODY" -g "$NOBODY" "${CREDENTIALS%/*}"
  share_log=$(mktemp)
  docker exec -u "$NOBODY:$NOBODY" "$container" /opt/ket/share_check \
    --address "$address" --credentials "$CREDENTIALS" >"$share_log" 2>&1 &
  share_pid=$!
  for attempt in $(seq 1 10); do
    grep -q '^sharing port=' "$share_log" && break
    sleep 1
  done
  grep -q '^sharing port=' "$share_log" || die "the share did not start: $(tr '\n' ' ' <"$share_log")"
  read -r port user password < <(lab cat "$CREDENTIALS")
  [[ $user == ket && ${#password} -eq 12 ]] || die 'the share wrote unexpected credentials'
  pass "sharing on $address:$port as an unprivileged user"
  start_leak_counter
  leak_counter_works
  refused_without_tunnel 'before connecting,'

  for protocol in "${protocols[@]}"; do
    run_protocol "$protocol" "$invite" "$first"
    first=false
  done
  guessing_is_stopped
  network_goes_away

  printf '\n'
  if ((failures > 0)); then
    printf 'SHARE FAILURES: %d check(s) failed.\n' "$failures"
    exit 1
  fi
  printf 'The share carried a nearby device through every protocol, and only through the tunnel.\n'
}

main "$@"
