#!/usr/bin/env bash
# Roaming lab: connects a disposable Ubuntu 24.04 desktop-like container to
# the canary with the shipped client code and moves it between networks
# mid-session, the way a laptop moves from Wi-Fi to Ethernet, to show the
# tunnel survives without leaking.
#
#   scripts/roaming-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...
#                          [--cache DIR] [--client FILE] [--deb FILE]
#
# The container starts on network A with network B attached but unused.
# For each protocol it:
#   1. connects and checks the server routes go through network A,
#   2. while a site is fetched every second, makes network B the default
#      route and removes network A,
#   3. checks the server routes moved to network B, the site loads again
#      within the recovery time, and nothing left network B outside the
#      tunnel,
#   4. removes every default route for a few seconds and checks the server
#      routes become unreachable rather than looping into the tunnel, then
#      restores network B and checks the site loads again,
#   5. disconnects and checks Ket's routes are gone.
# Whether the client had to reconnect (`phase=Reconnecting`) is reported;
# protocols that keep one long connection may need to.
# Protocols default to vless_xtls_reality. Needs Docker with privileged
# containers; the invite is revoked afterwards.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# shellcheck source=scripts/lib/desktop-lab.sh
. "$ROOT/scripts/lib/desktop-lab.sh"

readonly NETWORK_A=ket-roaming-lab-a
readonly NETWORK_B=ket-roaming-lab-b
readonly RECOVERY_SECONDS=20
readonly PROBE_URL=https://api.ipify.org

failures=0
container=''
client_pid=''
label=''

step() { printf '\n== %s\n' "$1"; }
pass() { printf '  ok    %s\n' "$1"; }
note() { printf '  note  %s\n' "$1"; }
problem() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
die() {
  printf 'roaming-lab: %s\n' "$1" >&2
  exit 2
}

ensure_network() {
  docker network inspect "$1" >/dev/null 2>&1 || docker network create "$1" >/dev/null
}

network_gateway() {
  docker network inspect "$1" --format '{{range .IPAM.Config}}{{.Gateway}}{{end}}'
}

# interface_on NETWORK: the container interface attached to that network.
interface_on() {
  local address
  address=$(docker inspect "$container" --format "{{(index .NetworkSettings.Networks \"$1\").IPAddress}}")
  lab ip -o -4 addr show | awk -v address="$address" 'index($4, address "/") == 1 { print $2; exit }'
}

ket_routes() { lab ip -4 route show proto 75; }

# probe_loop SECONDS EGRESS: asks the site for this computer's address every
# second inside the container, logging "EPOCH ok" only when the answer is
# EGRESS (the tunnel's exit) and "EPOCH fail" otherwise, to /tmp/probe.log.
probe_loop() {
  lab sh -c "for i in \$(seq 1 $1); do if [ \"\$(curl -4 -s -m 3 $PROBE_URL)\" = '$2' ]; then echo \"\$(date +%s) ok\"; else echo \"\$(date +%s) fail\"; fi; sleep 1; done > /tmp/probe.log 2>&1 &"
}

# loads_since EPOCH SECONDS: waits until the probe succeeds after EPOCH;
# prints how many seconds that took.
loads_since() {
  local since=$1 limit=$2 _ took
  for _ in $(seq 1 "$limit"); do
    took=$(lab cat /tmp/probe.log 2>/dev/null | awk -v since="$since" '$1 > since && $2 == "ok" { print $1 - since; exit }')
    if [[ -n $took ]]; then
      printf '%s\n' "$took"
      return 0
    fi
    sleep 1
  done
  return 1
}

# run_protocol PROTOCOL INVITE
run_protocol() {
  local protocol=$1 invite=$2 client_log interface_a interface_b gateway_b routes servers filter moved seconds leaked egress
  step "$protocol"
  start_desktop "ket-roaming-lab-$$" "$NETWORK_A" "$cache/engines" "$probe"
  docker network connect "$NETWORK_B" "$container"
  interface_a=$(interface_on "$NETWORK_A")
  interface_b=$(interface_on "$NETWORK_B")
  gateway_b=$(network_gateway "$NETWORK_B")
  client_log=$(mktemp)
  start_client ket-roaming-client "$client_log" "$protocol" "$invite" --maintain
  if ! wait_connected "$client_log" "$client_pid" 90; then
    if grep -q 'is not available for this invite' "$client_log"; then
      note "the canary does not offer $protocol; skipped"
    else
      problem "could not connect with $protocol within 90 s: $(tr '\n' ' ' <"$client_log")"
    fi
    finish_protocol "$client_log" skipped
    return
  fi
  routes=$(ket_routes)
  servers=$(awk '{ print $1 }' <<<"$routes" | sort -u | tr '\n' ' ')
  if [[ -n $servers ]] && ! grep -qv "dev $interface_a" <<<"$routes"; then
    pass "connected ($(grep '^connected' "$client_log")); server routes via network A: $servers"
  else
    problem "the server routes are not on network A ($interface_a): $(tr '\n' ';' <<<"$routes")"
  fi
  egress=$(lab curl -4 -s -m 15 "$PROBE_URL" || true)
  [[ -n $egress ]] || problem 'the site does not load through the tunnel before the move'
  # Anything on network B that is not Ket's own traffic to the server
  # leaked, from now until the client disconnects.
  filter="not arp and not (icmp6 and ip6[40] >= 133 and ip6[40] <= 137) and not udp port 5353"
  for server in $servers; do filter+=" and not host ${server%/32}"; done
  lab sh -c "tcpdump -i $interface_b -n --immediate-mode -U -w /tmp/leak.pcap '$filter' >/dev/null 2>&1 &"
  probe_loop 150 "$egress"
  sleep 3

  # Network A goes away; network B is the only way out.
  moved=$(date +%s)
  lab ip route replace default via "$gateway_b" dev "$interface_b"
  docker network disconnect "$NETWORK_A" "$container"
  if seconds=$(loads_since "$moved" "$RECOVERY_SECONDS"); then
    pass "after moving to network B the site loads again through the tunnel within $seconds s"
  else
    problem "after moving to network B the site did not load through the tunnel within $RECOVERY_SECONDS s"
  fi
  routes=$(ket_routes)
  if [[ -n $routes ]] && ! grep -qv "dev $interface_b" <<<"$routes"; then
    pass 'the server routes followed to network B'
  else
    problem "the server routes did not move to network B ($interface_b): $(tr '\n' ';' <<<"$routes")"
  fi

  # No network at all, then network B again.
  lab ip route del default || true
  sleep 5
  routes=$(ket_routes)
  if [[ -n $routes ]] && ! grep -qv '^unreachable' <<<"$routes"; then
    pass 'without any network the server routes are unreachable, not looped into the tunnel'
  else
    problem "without any network the server routes are: $(tr '\n' ';' <<<"$routes")"
  fi
  moved=$(date +%s)
  lab ip route replace default via "$gateway_b" dev "$interface_b"
  if seconds=$(loads_since "$moved" "$RECOVERY_SECONDS"); then
    pass "when the network came back the site loads again through the tunnel within $seconds s"
  else
    problem "when the network came back the site did not load through the tunnel within $RECOVERY_SECONDS s"
  fi
  # Packets of the last request may still be on their way to the capture.
  sleep 1
  lab pkill -INT tcpdump || true
  sleep 1
  leaked=$(lab sh -c 'tcpdump -n -r /tmp/leak.pcap 2>/dev/null | head -n 5')
  if [[ -z $leaked ]]; then
    pass 'nothing left network B outside the tunnel at any point'
  else
    problem "traffic left network B outside the tunnel: $(tr '\n' ';' <<<"$leaked")"
  fi
  if grep -q '^phase=Reconnecting' "$client_log"; then
    note "the client reconnected during the test ($(grep -c '^phase=Reconnecting' "$client_log") time(s))"
  else
    pass 'the client never had to reconnect'
  fi
  finish_protocol "$client_log"
}

# finish_protocol LOG [skipped]: disconnects and checks Ket's routes are gone.
finish_protocol() {
  local client_log=$1
  lab systemctl kill --signal=TERM ket-roaming-client.service 2>/dev/null || true
  wait "$client_pid" 2>/dev/null || true
  if [[ ${2:-} == skipped ]]; then :; elif grep -q '^disconnected' "$client_log"; then
    pass 'disconnected and released the session'
  else
    problem "disconnect failed: $(tr '\n' ' ' <"$client_log")"
  fi
  if [[ -z $(ket_routes) ]]; then pass "Ket's server routes are gone"; else
    problem "Ket's server routes were left behind: $(ket_routes | tr '\n' ';')"
  fi
  rm -f -- "$client_log"
  docker rm -f "$container" >/dev/null
  container=''
}

cleanup() {
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n $label ]] && ! canary_revoke "$label" >/dev/null; then
    printf 'roaming-lab: could not revoke the invite "%s"; revoke it with: sudo ket invites\n' "$label" >&2
  fi
}

main() {
  local protocol output invite deb=''
  local -a protocols=()
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab"
  probe=''
  [[ $# -ge 1 && $1 != -* ]] || { sed -n '2,25p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2; }
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
  ((${#protocols[@]})) || protocols=(vless_xtls_reality)
  for tool in docker cargo ssh jq; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  step 'Preparing the lab'
  fetch_engines "$cache/engines"
  [[ -z $deb ]] || engines_from_deb "$deb" "$cache/engines"
  [[ -n $probe ]] || probe=$(build_probe)
  build_image
  ensure_network "$NETWORK_A"
  ensure_network "$NETWORK_B"
  pass "image $DESKTOP_LAB_IMAGE, engines in $cache/engines, networks $NETWORK_A and $NETWORK_B"

  trap cleanup EXIT
  label="roaming lab $(git -C "$ROOT" rev-parse --short HEAD) $$"
  output=$(canary_invite "$label" 2) || die "ket invite failed on the canary: $output"
  invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || die 'the canary printed no invite'
  pass "invite from $canary_target ($label)"

  for protocol in "${protocols[@]}"; do
    run_protocol "$protocol" "$invite"
  done

  printf '\n'
  if ((failures > 0)); then
    printf 'ROAMING FAILURES: %d check(s) failed.\n' "$failures"
    exit 1
  fi
  printf 'Every protocol survived the network changes without a leak.\n'
}

main "$@"
