#!/usr/bin/env bash
# DNS leak lab: connects a disposable Ubuntu 24.04 desktop-like container to
# the canary with the shipped client code, under the shipped tunnel service's
# systemd sandbox, and runs scripts/dns-leak-check.sh inside it, so a release can show that no lookup leaves outside the tunnel.
#
#   scripts/dns-leak-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...
#                           [--resolver resolved|plain]... [--cache DIR]
#                           [--client FILE] [--deb FILE]
#
# For each resolver setup and protocol it:
#   1. checks the capture sees direct lookups without Ket (baseline),
#   2. connects with an invite from the canary and runs the leak check,
#   3. for systemd-resolved, changes the network's DNS servers mid-session
#      (what a DHCP renewal does) and checks again,
#   4. disconnects and checks the resolver configuration is back.
#
# resolved: systemd-resolved owns /etc/resolv.conf, the network supplies this
#           computer's resolver and a link-local IPv6 resolver (Ubuntu and
#           Fedora desktops).
# plain:    no systemd-resolved; /etc/resolv.conf names the network's resolver.
# Protocols default to vless_xtls_reality. --client tests another build of
# the connect_check example (such as an older commit's) instead of this
# tree's. OpenVPN and stunnel are built from source for packages, so the lab
# takes them from a Ket .deb given with --deb (then open_vpn_stunnel works). Needs Docker with privileged containers; the invite is revoked
# afterwards.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"

readonly IMAGE=ket-dns-leak-lab:ubuntu-24.04
readonly NETWORK=ket-dns-leak-lab
readonly SUBNET6=fd6b:6574:6c61::/64
readonly LINK_LOCAL_RESOLVER=fe80::1
# The resolver a DHCP renewal switches to mid-session.
readonly RENEWED_RESOLVER=9.9.9.9
readonly ENGINES=(xray tun2proxy hysteria sslocal wstunnel)

failures=0
container=''
label=''
network_resolver=''

step() { printf '\n== %s\n' "$1"; }
pass() { printf '  ok    %s\n' "$1"; }
problem() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
die() {
  printf 'dns-leak-lab: %s\n' "$1" >&2
  exit 2
}

lab() { docker exec "$container" "$@"; }

# unit_sandbox: the shipped tunnel service's sandbox properties, one
# KEY=VALUE per line, so the client runs under the same restrictions as on a
# desktop.
unit_sandbox() {
  sed -n '/^\[Service\]/,/^\[/p' "$ROOT/packaging/linux/ket-tunnel.service" \
    | grep -E '^[A-Za-z]+=' \
    | grep -vE '^(Type|User|Group|Exec[A-Za-z]*|Kill[A-Za-z]*|Restart[A-Za-z]*|Timeout[A-Za-z]*)='
}

fetch_engines() {
  local cache=$1 engine
  mkdir -p "$cache"
  for engine in "${ENGINES[@]}"; do
    [[ -x $cache/$engine ]] && continue
    case $engine in
      sslocal) "$ROOT/packaging/fetch-shadowsocks.sh" linux-amd64 "$cache/sslocal" ;;
      *) "$ROOT/packaging/fetch-$engine.sh" linux-amd64 "$cache/$engine" ;;
    esac
  done
}

# engines_from_deb DEB DIRECTORY: OpenVPN and stunnel from a Ket package.
engines_from_deb() {
  local unpacked engine
  unpacked=$(mktemp -d)
  dpkg-deb -x "$1" "$unpacked"
  for engine in openvpn stunnel; do
    install -m 0755 "$unpacked/usr/libexec/ket/$engine" "$2/$engine"
  done
  rm -rf -- "$unpacked"
}

build_image() {
  docker build -q -t "$IMAGE" - >/dev/null <<'DOCKERFILE'
FROM ubuntu:24.04
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      systemd systemd-sysv systemd-resolved dbus tcpdump iproute2 dnsutils curl ca-certificates procps \
 && rm -rf /var/lib/apt/lists/* \
 && systemctl mask systemd-networkd.service systemd-networkd.socket getty.target console-getty.service
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
DOCKERFILE
}

ensure_network() {
  docker network inspect "$NETWORK" >/dev/null 2>&1 \
    || docker network create --ipv6 --subnet "$SUBNET6" "$NETWORK" >/dev/null
}

start_container() {
  local engines=$1 probe=$2
  container=ket-dns-leak-lab-$$
  docker run -d --name "$container" --privileged --cgroupns=private --network "$NETWORK" \
    --tmpfs /run --tmpfs /run/lock \
    -v "$engines:/opt/ket/engines:ro" -v "$probe:/opt/ket/connect_check:ro" \
    -v "$ROOT/scripts/dns-leak-check.sh:/opt/ket/dns-leak-check.sh:ro" \
    "$IMAGE" >/dev/null
  local attempt
  for attempt in $(seq 1 30); do
    [[ $(lab systemctl is-active systemd-resolved 2>/dev/null) == active ]] && return 0
    sleep 1
  done
  die "systemd-resolved did not start in the lab container (after $attempt s)"
}

# host_resolver: the resolver this computer's network hands out, which the
# lab container uses as its own network's resolver (some networks block
# public resolvers).
host_resolver() {
  local resolver
  resolver=$(awk '$1 == "nameserver" && $2 !~ /^(127\.|::1)/ { print $2; exit }' /etc/resolv.conf /run/systemd/resolve/resolv.conf 2>/dev/null || true)
  printf '%s\n' "${resolver:-$RENEWED_RESOLVER}"
}

# use_resolver MODE: makes the container's resolver look like that desktop.
use_resolver() {
  case $1 in
    resolved)
      lab sh -c 'umount /etc/resolv.conf 2>/dev/null; ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf'
      lab resolvectl dns eth0 "$network_resolver" "$LINK_LOCAL_RESOLVER"
      ;;
    plain)
      lab systemctl disable --now -q systemd-resolved
      lab sh -c "umount /etc/resolv.conf 2>/dev/null; rm -f /etc/resolv.conf; printf 'nameserver %s\n' $network_resolver > /etc/resolv.conf"
      ;;
  esac
}

resolver_snapshot() {
  lab sh -c 'cat /etc/resolv.conf; command -v resolvectl >/dev/null && resolvectl dns 2>/dev/null' || true
}

leak_check() {
  local description=$1 output
  shift
  if output=$(lab /opt/ket/dns-leak-check.sh "$@" 2>&1); then
    pass "$description: no leak"
  else
    problem "$description:"
    awk '{ print "          " $0 }' <<<"$output"
  fi
}

baseline_check() {
  local output
  if output=$(lab /opt/ket/dns-leak-check.sh --baseline 2>&1); then
    problem "without Ket the capture saw no direct lookup, so it cannot see a leak here: $output"
  elif grep -Fq 'probe lookup left' <<<"$output"; then
    pass 'without Ket the capture sees direct lookups (baseline)'
  else
    problem "the baseline check could not run: $output"
  fi
}

# run_protocol MODE PROTOCOL INVITE
run_protocol() {
  local mode=$1 protocol=$2 invite=$3 before after attempt egress client_log property
  local -a sandbox=()
  step "$mode resolver, $protocol"
  client_log=$(mktemp)
  while IFS= read -r property; do sandbox+=(-p "$property"); done < <(unit_sandbox)
  before=$(resolver_snapshot)
  printf '%s\n' "$invite" | docker exec -i "$container" systemd-run --quiet --wait --pipe --collect \
    --unit ket-leak-client "${sandbox[@]}" \
    /opt/ket/connect_check --engines /opt/ket/engines --runtime /run/ket --protocol "$protocol" \
    >"$client_log" 2>&1 &
  local client=$!
  for attempt in $(seq 1 90); do
    grep -q '^connected' "$client_log" && break
    kill -0 "$client" 2>/dev/null || break
    sleep 1
  done
  if ! grep -q '^connected' "$client_log"; then
    problem "could not connect with $protocol within $attempt s: $(tr '\n' ' ' <"$client_log")"
    lab systemctl kill --signal=TERM ket-leak-client.service 2>/dev/null || true
    wait "$client" 2>/dev/null || true
    return
  fi
  egress=$(lab curl -4 -s -m 15 https://api.ipify.org || true)
  if [[ -n $egress ]]; then
    pass "connected ($(grep '^connected' "$client_log")); a site by name loads, egress address $egress"
  else
    problem "connected ($(grep '^connected' "$client_log")), but a site by name does not load through the tunnel"
  fi
  leak_check 'lookups while connected'
  if [[ $mode == resolved ]]; then
    lab resolvectl dns eth0 "$RENEWED_RESOLVER" "$LINK_LOCAL_RESOLVER"
    sleep 2
    leak_check 'lookups after the network changed its DNS servers'
  fi
  lab systemctl kill --signal=TERM ket-leak-client.service || true
  wait "$client" 2>/dev/null || true
  if grep -q '^disconnected' "$client_log"; then pass 'disconnected and released the session'; else
    problem "disconnect failed: $(tr '\n' ' ' <"$client_log")"
  fi
  rm -f -- "$client_log"
  if [[ $mode == resolved ]]; then
    lab resolvectl dns eth0 "$network_resolver" "$LINK_LOCAL_RESOLVER"
  fi
  after=$(resolver_snapshot)
  if [[ $before == "$after" ]]; then pass 'the resolver configuration is back as it was'; else
    problem "the resolver configuration changed: before [$(tr '\n' ' ' <<<"$before")] after [$(tr '\n' ' ' <<<"$after")]"
  fi
}

cleanup() {
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n $label ]] && ! canary_revoke "$label" >/dev/null; then
    printf 'dns-leak-lab: could not revoke the invite "%s"; revoke it with: sudo ket invites\n' "$label" >&2
  fi
}

main() {
  local cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab" mode protocol output invite probe='' deb=''
  local -a protocols=() modes=()
  [[ $# -ge 1 && $1 != -* ]] || { sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2; }
  canary_target=$1
  canary_ssh_options=()
  shift
  while (($#)); do
    case $1 in
      --ssh-key) [[ $# -ge 2 ]] || die '--ssh-key needs a file'; canary_ssh_options+=(-i "$2" -o IdentitiesOnly=yes); shift 2 ;;
      --protocol) [[ $# -ge 2 ]] || die '--protocol needs a name'; protocols+=("$2"); shift 2 ;;
      --resolver) [[ $# -ge 2 && $2 =~ ^(resolved|plain)$ ]] || die '--resolver is resolved or plain'; modes+=("$2"); shift 2 ;;
      --cache) [[ $# -ge 2 ]] || die '--cache needs a directory'; cache=$2; shift 2 ;;
      --client) [[ $# -ge 2 && -f $2 ]] || die '--client needs a connect_check binary'; probe=$(realpath "$2"); shift 2 ;;
      --deb) [[ $# -ge 2 && -f $2 ]] || die '--deb needs a Ket .deb'; deb=$2; shift 2 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  ((${#protocols[@]})) || protocols=(vless_xtls_reality)
  ((${#modes[@]})) || modes=(resolved plain)
  for tool in docker cargo ssh jq; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  step 'Preparing the lab'
  fetch_engines "$cache/engines"
  [[ -z $deb ]] || engines_from_deb "$deb" "$cache/engines"
  if [[ -z $probe ]]; then
    (cd "$ROOT" && cargo build -q -p ket-client-core --example connect_check)
    probe=$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/debug/examples/connect_check
  fi
  build_image
  ensure_network
  network_resolver=$(host_resolver)
  pass "image $IMAGE, engines in $cache/engines, network resolver $network_resolver"

  trap cleanup EXIT
  # Sessions run one at a time and each is released; the spare device covers
  # a release that is still in flight.
  output=$(canary_invite "dns leak lab $(git -C "$ROOT" rev-parse --short HEAD) $$" 2) \
    || die "ket invite failed on the canary: $output"
  label="dns leak lab $(git -C "$ROOT" rev-parse --short HEAD) $$"
  invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || die 'the canary printed no invite'
  pass "invite from $canary_target ($label)"

  for mode in "${modes[@]}"; do
    start_container "$cache/engines" "$probe"
    use_resolver "$mode"
    step "$mode resolver, without Ket"
    baseline_check
    for protocol in "${protocols[@]}"; do
      run_protocol "$mode" "$protocol" "$invite"
    done
    docker rm -f "$container" >/dev/null
    container=''
  done

  printf '\n'
  if ((failures > 0)); then
    printf 'LEAKS OR FAILURES: %d check(s) failed.\n' "$failures"
    exit 1
  fi
  printf 'No DNS leak in any setup.\n'
}

main "$@"
