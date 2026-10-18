#!/usr/bin/env bash
# Reachability lab: connects to a Ket server with every protocol, one at a
# time, from wherever it runs, and reports which ones carry traffic. Run it
# from a network that blocks nothing (a GitHub runner, a cloud machine) to
# tell a network that blocks a protocol apart from a fault on the server.
#
#   scripts/reachability-lab.sh (--invite-file FILE | SSH_TARGET [--ssh-key FILE])
#                               [--protocol NAME]... [--cache DIR] [--client FILE]
#                               [--deb FILE | --build-openvpn]
#
# With SSH_TARGET it creates a short-lived invite on that server and revokes
# it afterwards; with --invite-file it uses the invite in FILE (or "-" for
# stdin), which it never prints. Each protocol runs in a fresh disposable
# Ubuntu 24.04 container with the shipped client code under the tunnel
# service's sandbox, and passes when a site loads through the tunnel.
# OpenVPN needs OpenVPN and stunnel from a Ket .deb (--deb) or built from
# source (--build-openvpn); without either it is skipped. Output names no
# server address or host name, so it is safe for public CI logs. Exits 1
# when a protocol the server offers fails. Needs Docker with privileged
# containers.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# shellcheck source=scripts/lib/desktop-lab.sh
. "$ROOT/scripts/lib/desktop-lab.sh"

readonly ALL_PROTOCOLS=(vless_xtls_reality stealth hysteria2 xor_scrambled shadowsocks2022 wire_guard open_vpn_stunnel)
readonly CONNECT_SECONDS=90
readonly PROBE_URL=https://www.cloudflare.com/cdn-cgi/trace

container=''
client_pid=''
label=''
results=()
failures=0

step() { printf '\n== %s\n' "$1"; }
die() {
  printf 'reachability-lab: %s\n' "$1" >&2
  exit 2
}

# redact: removes URLs, addresses, and host names from text on stdin, so a
# failure reason never names the server.
redact() {
  sed -E \
    -e 's#[A-Za-z][A-Za-z0-9+.-]*://[^[:space:]"]+#[url]#g' \
    -e 's#\[[0-9A-Fa-f:.]+(%[[:alnum:]_.-]+)?\](:[0-9]+)?#[address]#g' \
    -e 's#\b[0-9]{1,3}(\.[0-9]{1,3}){3}(:[0-9]+)?\b#[address]#g' \
    -e 's#([0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}#[address]#g' \
    -e 's#\b([A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+[A-Za-z][A-Za-z0-9-]{0,61}[A-Za-z0-9](:[0-9]+)?\b#[host]#g' \
    | tr '\n' ' ' | cut -c1-300
}

# not_offered PROTOCOL: why a protocol the server does not offer was skipped.
# A server runs Hysteria 2 either plain or with XOR obfuscation, never both.
not_offered() {
  case $1 in
    hysteria2) printf 'not offered (this server runs Hysteria 2 with XOR obfuscation, or not at all)' ;;
    xor_scrambled) printf 'not offered (this server runs plain Hysteria 2, or not at all)' ;;
    *) printf 'not offered by the server' ;;
  esac
}

record() { results+=("$(printf '%-20s %-9s %s' "$1" "$2" "$3")"); }

# run_protocol PROTOCOL INVITE
run_protocol() {
  local protocol=$1 invite=$2 client_log started seconds direct tunneled reason
  step "$protocol"
  if [[ $protocol == open_vpn_stunnel && ! ( -x $cache/engines/openvpn && -x $cache/engines/stunnel ) ]]; then
    printf '  skipped: OpenVPN needs --deb or --build-openvpn\n'
    record "$protocol" skipped 'no OpenVPN engine in this lab'
    return
  fi
  start_desktop "ket-reachability-lab-$$" bridge "$cache/engines" "$probe"
  direct=$(lab curl -4 -s -m 15 "$PROBE_URL" | sed -n 's/^ip=//p' || true)
  client_log=$(mktemp)
  started=$(date +%s)
  start_client ket-reachability-client "$client_log" "$protocol" "$invite"
  if wait_connected "$client_log" "$client_pid" "$CONNECT_SECONDS"; then
    seconds=$(($(date +%s) - started))
    tunneled=$(lab curl -4 -s -m 20 "$PROBE_URL" | sed -n 's/^ip=//p' || true)
    if [[ -n $tunneled && $tunneled != "$direct" ]]; then
      printf '  works: connected in %s s and a site loads through the tunnel\n' "$seconds"
      record "$protocol" works "connected in $seconds s"
    else
      printf '  FAILS: connected in %s s, but no site loads through the tunnel\n' "$seconds"
      record "$protocol" FAILS 'connected, but no traffic passes'
      failures=$((failures + 1))
    fi
  elif grep -q 'is not available for this invite' "$client_log"; then
    printf '  skipped: %s\n' "$(not_offered "$protocol")"
    record "$protocol" skipped "$(not_offered "$protocol")"
  else
    reason=$(grep -m 1 '^connect_check:' "$client_log" | sed 's/^connect_check: //' | redact)
    [[ -n $reason ]] || reason="no connection within $CONNECT_SECONDS s"
    printf '  FAILS: %s\n' "$reason"
    record "$protocol" FAILS "$reason"
    failures=$((failures + 1))
  fi
  lab systemctl kill --signal=TERM ket-reachability-client.service 2>/dev/null || true
  wait "$client_pid" 2>/dev/null || true
  rm -f -- "$client_log"
  docker rm -f "$container" >/dev/null
  container=''
}

cleanup() {
  [[ -z $container ]] || docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n $label ]] && ! canary_revoke "$label" >/dev/null; then
    printf 'reachability-lab: could not revoke the invite "%s"; revoke it with: sudo ket invites\n' "$label" >&2
  fi
}

# build_openvpn DIRECTORY: OpenVPN and stunnel built from source, as the
# Linux package ships them.
build_openvpn() {
  [[ -x $1/openvpn ]] || "$ROOT/packaging/fetch-openvpn.sh" linux-amd64 "$1/openvpn"
  [[ -x $1/stunnel ]] || "$ROOT/packaging/fetch-stunnel.sh" linux-amd64 "$1/stunnel"
}

main() {
  local protocol output invite='' invite_file='' deb='' build_ovpn=false
  local -a protocols=()
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/ket-dns-leak-lab"
  probe=''
  canary_target=''
  canary_ssh_options=()
  if [[ $# -ge 1 && $1 != -* ]]; then
    canary_target=$1
    shift
  fi
  while (($#)); do
    case $1 in
      --invite-file) [[ $# -ge 2 ]] || die '--invite-file needs a file'; invite_file=$2; shift 2 ;;
      --ssh-key) [[ $# -ge 2 ]] || die '--ssh-key needs a file'; canary_ssh_options+=(-i "$2" -o IdentitiesOnly=yes); shift 2 ;;
      --protocol) [[ $# -ge 2 ]] || die '--protocol needs a name'; protocols+=("$2"); shift 2 ;;
      --cache) [[ $# -ge 2 ]] || die '--cache needs a directory'; cache=$2; shift 2 ;;
      --client) [[ $# -ge 2 && -f $2 ]] || die '--client needs a connect_check binary'; probe=$(realpath "$2"); shift 2 ;;
      --deb) [[ $# -ge 2 && -f $2 ]] || die '--deb needs a Ket .deb'; deb=$2; shift 2 ;;
      --build-openvpn) build_ovpn=true; shift ;;
      -h|--help) sed -n '2,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  [[ -n $canary_target || -n $invite_file ]] || die 'give an SSH target or --invite-file (see --help)'
  [[ -z $canary_target || -z $invite_file ]] || die 'give an SSH target or --invite-file, not both'
  ((${#protocols[@]})) || protocols=("${ALL_PROTOCOLS[@]}")
  for tool in docker cargo jq; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  step 'Preparing the lab'
  fetch_engines "$cache/engines"
  if [[ -n $deb ]]; then engines_from_deb "$deb" "$cache/engines"
  elif $build_ovpn; then build_openvpn "$cache/engines"
  fi
  [[ -n $probe ]] || probe=$(build_probe)
  build_image
  printf '  ok    image %s, engines in %s\n' "$DESKTOP_LAB_IMAGE" "$cache/engines"

  trap cleanup EXIT
  if [[ -n $invite_file ]]; then
    if [[ $invite_file == - ]]; then invite=$(cat); else invite=$(cat -- "$invite_file"); fi
    invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$invite") || die 'no ket:invite: line in the invite file'
    printf '  ok    invite from the file\n'
  else
    command -v ssh >/dev/null 2>&1 || die 'ssh is required'
    label="reachability lab $(git -C "$ROOT" rev-parse --short HEAD) $$"
    output=$(canary_invite "$label" 1) || die "ket invite failed on the server: $(redact <<<"$output")"
    invite=$(grep -m 1 -o 'ket:invite:[^[:space:]]*' <<<"$output") || die 'the server printed no invite'
    printf '  ok    invite from the server (%s)\n' "$label"
  fi

  for protocol in "${protocols[@]}"; do
    run_protocol "$protocol" "$invite"
  done

  step 'Summary'
  printf '%-20s %-9s %s\n' PROTOCOL RESULT DETAIL
  printf '%s\n' "${results[@]}"
  if ((failures > 0)); then
    printf '\n%d protocol(s) the server offers failed from this network.\n' "$failures"
    exit 1
  fi
  printf '\nEvery protocol the server offers works from this network.\n'
}

main "$@"
