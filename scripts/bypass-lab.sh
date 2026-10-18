#!/usr/bin/env bash
# Bypass lab: connects a disposable Ubuntu 24.04 desktop-like container to
# the canary with the shipped client code and a bypass list, and checks that
# the listed address is reached directly while everything else, DNS
# included, still goes through the tunnel.
#
#   scripts/bypass-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...
#                         [--cache DIR] [--client FILE] [--deb FILE]
#
# For each protocol it connects with two bypass entries: 1.1.1.1, and the
# container's own network. Then it checks that
#   1. a site reached by its address through the bypass sees the container's
#      own address, and a site reached by name sees the tunnel's exit;
#   2. 1.1.1.1 has a Ket route through the container's network, and the
#      container's own network has none (it is already reached directly);
#   3. nothing but the server and 1.1.1.1 left the container's interface
#      outside the tunnel, so no DNS query leaked;
#   4. Ket's routes are gone after disconnecting.
# Protocols default to vless_xtls_reality. Needs Docker with privileged
# containers; the invite is revoked afterwards.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# shellcheck source=scripts/lib/desktop-lab.sh
. "$ROOT/scripts/lib/desktop-lab.sh"

readonly NETWORK=ket-bypass-lab
readonly BYPASSED=1.1.1.1
readonly BYPASSED_URL=https://1.1.1.1/cdn-cgi/trace
readonly TUNNELED_URL=https://www.cloudflare.com/cdn-cgi/trace

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
  printf 'bypass-lab: %s\n' "$1" >&2
  exit 2
}

ket_routes() { lab ip -4 route show proto 75; }
egress() { lab curl -4 -s -m 15 "$1" | sed -n 's/^ip=//p' || true; }

# run_protocol PROTOCOL INVITE SUBNET
run_protocol() {
  local protocol=$1 invite=$2 subnet=$3 client_log interface direct routes servers filter leaked seen
  step "$protocol"
  start_desktop "ket-bypass-lab-$$" "$NETWORK" "$cache/engines" "$probe"
  interface=$(lab ip -o -4 route show default | awk '{ print $5; exit }')
  direct=$(egress "$BYPASSED_URL")
  [[ -n $direct ]] || problem 'the container cannot reach the bypassed site before connecting'
  client_log=$(mktemp)
  start_client ket-bypass-client "$client_log" "$protocol" "$invite" --bypass "$BYPASSED" --bypass "$subnet"
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
  servers=$(awk '$1 != "'"$BYPASSED"'" && $1 != "'"$subnet"'" { print $1 }' <<<"$routes" | sort -u | tr '\n' ' ')
  # Everything that is not Ket's own traffic to the server or the bypassed
  # address leaked, from now until the client disconnects.
  # Broadcast and multicast never leave the network (the host's own
  # discovery traffic reaches the bridge too), so they do not count.
  filter="not arp and not broadcast and not multicast and not (icmp6 and ip6[40] >= 133 and ip6[40] <= 137) and not udp port 5353 and not host $BYPASSED"
  for server in $servers; do filter+=" and not host ${server%/32}"; done
  lab sh -c "tcpdump -i $interface -n --immediate-mode -U -w /tmp/leak.pcap '$filter' >/dev/null 2>&1 &"
  sleep 2

  seen=$(egress "$BYPASSED_URL")
  if [[ -n $seen && $seen == "$direct" ]]; then
    pass "the bypassed address is reached directly (it sees the container's own address)"
  else
    problem "the bypassed address did not see the container's own address"
  fi
  seen=$(egress "$TUNNELED_URL")
  if [[ -n $seen && $seen != "$direct" ]]; then
    pass 'a site reached by name goes through the tunnel'
  else
    problem 'a site reached by name did not go through the tunnel'
  fi
  lab getent hosts example.com >/dev/null 2>&1 || problem 'names no longer resolve while connected'
  if grep -q "^$BYPASSED via .* dev $interface" <<<"$routes"; then
    pass "$BYPASSED has a Ket route through $interface"
  else
    problem "$BYPASSED has no Ket route through $interface: $(tr '\n' ';' <<<"$routes")"
  fi
  if grep -q "^$subnet " <<<"$routes"; then
    problem "the container's own network $subnet got a Ket route; it is reached directly already"
  else
    pass "the container's own network $subnet is left to its own route"
  fi
  # Packets of the last request may still be on their way to the capture.
  sleep 1
  lab pkill -INT tcpdump || true
  sleep 1
  leaked=$(lab sh -c 'tcpdump -n -r /tmp/leak.pcap 2>/dev/null | head -n 5')
  if [[ -z $leaked ]]; then
    pass 'nothing else left outside the tunnel, DNS included'
  else
    problem "traffic left outside the tunnel: $(tr '\n' ';' <<<"$leaked")"
  fi
  finish_protocol "$client_log"
}

# finish_protocol LOG [skipped]: disconnects and checks Ket's routes are gone.
finish_protocol() {
  local client_log=$1
  lab systemctl kill --signal=TERM ket-bypass-client.service 2>/dev/null || true
  wait "$client_pid" 2>/dev/null || true
  if [[ ${2:-} == skipped ]]; then :; elif grep -q '^disconnected' "$client_log"; then
    pass 'disconnected and released the session'
  else
    problem "disconnect failed: $(tr '\n' ' ' <"$client_log")"
  fi
  if [[ -z $(ket_routes) ]]; then pass "Ket's routes are gone"; else
    problem "Ket's routes were left behind: $(ket_routes | tr '\n' ';')"
  fi
  rm -f -- "$client_log"
  docker rm -f "$container" >/dev/null
  container=''
}

cleanup() {
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n $label ]] && ! canary_revoke "$label" >/dev/null; then
    printf 'bypass-lab: could not revoke the invite "%s"; revoke it with: sudo ket invites\n' "$label" >&2
  fi
}

main() {
  local protocol output invite subnet deb=''
  local -a protocols=()
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab"
  probe=''
  [[ $# -ge 1 && $1 != -* ]] || { sed -n '2,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2; }
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
  docker network inspect "$NETWORK" >/dev/null 2>&1 || docker network create "$NETWORK" >/dev/null
  subnet=$(docker network inspect "$NETWORK" --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}')
  pass "image $DESKTOP_LAB_IMAGE, engines in $cache/engines, network $NETWORK ($subnet)"

  trap cleanup EXIT
  label="bypass lab $(git -C "$ROOT" rev-parse --short HEAD) $$"
  output=$(canary_invite "$label" 1) || die "ket invite failed on the canary: $output"
  invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || die 'the canary printed no invite'
  pass "invite from $canary_target ($label)"

  for protocol in "${protocols[@]}"; do
    run_protocol "$protocol" "$invite" "$subnet"
  done

  printf '\n'
  if ((failures > 0)); then
    printf 'BYPASS FAILURES: %d check(s) failed.\n' "$failures"
    exit 1
  fi
  printf 'Every protocol kept the bypass list outside the tunnel without a leak.\n'
}

main "$@"
