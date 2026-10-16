#!/usr/bin/env bash
# DNS and IPv6 leak check for a Linux computer while Ket is connected.
#
#   sudo scripts/dns-leak-check.sh [--allow ADDRESS]... [--seconds N] [--baseline]
#
# Captures every network interface except the Ket tunnel and loopback, looks
# up unique names through each resolver path the system offers (glibc, the
# systemd-resolved API, and direct resolv.conf tools), opens an IPv6
# connection, then reports anything that left outside the tunnel:
#
#   - a probe name in any packet (DNS, mDNS, or LLMNR),
#   - any other plaintext or encrypted DNS (ports 53 and 853),
#   - IPv6 traffic to a non-local address.
#
# --allow names an address that may be reached directly, such as the Ket
# server the engines connect to. --baseline runs without Ket and watches every
# interface, so it must report the probes: proof that the capture sees what a
# leak would look like here. Exit status: 0 no leak, 1 leak, 2 the check could
# not run.
set -euo pipefail

readonly PROBE_DOMAIN=example.com
readonly IPV6_PROBE=2606:4700:4700::1111

say() { printf '%s\n' "$*"; }
die() {
  printf 'dns-leak-check: %s\n' "$1" >&2
  exit 2
}

# --- analysis (pure: reads `tcpdump -n -r` lines prefixed with "IFACE ") -------

# An address with port 53 or 853 (tcpdump prints IPv4 ports as a fifth dotted
# field and IPv6 ports after a dot).
readonly DNS_PORT_PATTERN='[[:space:]](([0-9]+\.){4}|[0-9a-fA-F]*:[0-9a-fA-F:]*\.)(53|853)[:[:space:]]'

# leak_findings NONCE ALLOWED...: one "reason: packet" line per leaked packet.
leak_findings() {
  local nonce=$1 line interface packet allowed address
  shift
  while IFS= read -r line; do
    interface=${line%% *}
    packet=${line#* }
    allowed=false
    for address in "$@"; do
      [[ $packet == *" $address."* || $packet == *" $address:"* ]] && allowed=true
    done
    if [[ $packet == *"$nonce"* ]]; then
      printf 'probe lookup left through %s: %s\n' "$interface" "$packet"
    elif $allowed; then
      continue
    elif [[ $packet =~ $DNS_PORT_PATTERN ]]; then
      printf 'DNS outside the tunnel on %s: %s\n' "$interface" "$packet"
    elif [[ $packet =~ (^|[[:space:]])IP6[[:space:]] ]] && ! local_ipv6_packet "$packet"; then
      printf 'IPv6 outside the tunnel on %s: %s\n' "$interface" "$packet"
    fi
  done
}

# local_ipv6_packet PACKET: true for link-local or multicast IPv6, which never
# leaves the local network.
local_ipv6_packet() {
  local destination
  destination=$(sed -n 's/.* > \([^ ]*\): .*/\1/p' <<<"$1")
  [[ $destination == fe8* || $destination == fe9* || $destination == fea* || $destination == feb* \
    || $destination == ff* ]]
}

# --- the live check ------------------------------------------------------------

tunnel_interface() {
  local device
  device=$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* dev \([^ ]*\).*/\1/p' | head -n 1)
  [[ -n $device && -e /sys/class/net/$device/tun_flags ]] && printf '%s\n' "$device"
}

outside_interfaces() {
  local tunnel=$1 path name
  for path in /sys/class/net/*; do
    name=${path##*/}
    [[ $name == lo || $name == "$tunnel" ]] && continue
    [[ $(cat "$path/operstate" 2>/dev/null) == down ]] && continue
    printf '%s\n' "$name"
  done
}

# probe_lookups NONCE: looks up unique names through every resolver path here.
probe_lookups() {
  local nonce=$1 name
  name="$nonce-glibc.$PROBE_DOMAIN"
  say "  glibc (getaddrinfo, A and AAAA): $name"
  timeout 8 getent ahosts "$name" >/dev/null 2>&1 || true
  timeout 8 getent ahostsv6 "$nonce-glibc6.$PROBE_DOMAIN" >/dev/null 2>&1 || true
  if command -v resolvectl >/dev/null 2>&1 && systemctl is-active --quiet systemd-resolved 2>/dev/null; then
    name="$nonce-resolved.$PROBE_DOMAIN"
    say "  systemd-resolved API: $name"
    timeout 8 resolvectl query --cache=no "$name" >/dev/null 2>&1 || true
  fi
  if command -v dig >/dev/null 2>&1; then
    name="$nonce-dig.$PROBE_DOMAIN"
    say "  resolv.conf (dig): $name"
    timeout 8 dig +tries=1 +time=3 "$name" A >/dev/null 2>&1 || true
    timeout 8 dig +tries=1 +time=3 "$nonce-dig6.$PROBE_DOMAIN" AAAA >/dev/null 2>&1 || true
  fi
  if command -v curl >/dev/null 2>&1; then
    say "  IPv6 connection: [$IPV6_PROBE]:443"
    timeout 8 curl -6 -s -o /dev/null -m 6 "https://[$IPV6_PROBE]/" >/dev/null 2>&1 || true
  fi
}

scratch=''
capture_pids=()

cleanup() {
  ((${#capture_pids[@]} == 0)) || kill "${capture_pids[@]}" 2>/dev/null || true
  [[ -z $scratch ]] || rm -rf -- "$scratch"
}

main() {
  local seconds=2 baseline=false tunnel nonce interface findings
  local -a allowed=() interfaces=()
  while (($#)); do
    case $1 in
      --allow) [[ $# -ge 2 ]] || die '--allow needs an address'; allowed+=("$2"); shift 2 ;;
      --seconds) [[ $# -ge 2 && $2 =~ ^[0-9]+$ ]] || die '--seconds needs a number'; seconds=$2; shift 2 ;;
      --baseline) baseline=true; shift ;;
      -h | --help) sed -n '2,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  [[ $(id -u) -eq 0 ]] || die 'run as root (packet capture needs it)'
  for tool in ip tcpdump getent timeout; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done
  if $baseline; then
    tunnel=none
  else
    tunnel=$(tunnel_interface) || die 'traffic does not go through a tunnel; connect Ket first'
  fi
  mapfile -t interfaces < <(outside_interfaces "$tunnel")
  ((${#interfaces[@]} > 0)) || die 'no network interface outside the tunnel to watch'

  scratch=$(mktemp -d)
  trap cleanup EXIT
  nonce="ket-leak-$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
  say "Tunnel: $tunnel. Watching: ${interfaces[*]}."
  for interface in "${interfaces[@]}"; do
    tcpdump -i "$interface" -n -U -s 0 -w "$scratch/$interface.pcap" 2>/dev/null &
    capture_pids+=("$!")
  done
  sleep 1
  say 'Probes:'
  probe_lookups "$nonce"
  sleep "$seconds"
  kill -INT "${capture_pids[@]}" 2>/dev/null || true
  wait "${capture_pids[@]}" 2>/dev/null || true
  capture_pids=()

  findings=$(for interface in "${interfaces[@]}"; do
    tcpdump -n -r "$scratch/$interface.pcap" 2>/dev/null | sed "s/^/$interface /"
  done | leak_findings "$nonce" "${allowed[@]}")
  if [[ -n $findings ]]; then
    say ''
    say "LEAK: traffic left outside the tunnel:"
    awk '{ print "  " $0 }' <<<"$findings"
    exit 1
  fi
  say ''
  say "No leak: no probe lookup, DNS, or IPv6 traffic left outside $tunnel."
}

[[ -n ${DNS_LEAK_CHECK_LIBRARY:-} ]] || main "$@"
