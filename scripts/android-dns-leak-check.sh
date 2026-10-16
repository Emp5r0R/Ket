#!/usr/bin/env bash
# DNS and IPv6 leak check for Ket on an Android emulator.
#
#   scripts/android-dns-leak-check.sh [--serial emulator-5554] [--baseline]
#
# Turn the emulator's Wi-Fi off (`adb shell svc wifi disable`), then connect
# Ket. The emulator's cellular network card is captured from the host
# (`adb emu network capture`, which does not see the Wi-Fi card), so this sees
# exactly what leaves the device, as a network observer would. Lookups of unique names
# and an IPv6 ping run inside the device as the shell user, which the VPN
# covers when every app uses Ket. The verdict is scripts/dns-leak-check.sh's.
# --baseline runs without Ket and must report the probes.
#
# Exit status: 0 no leak, 1 leak, 2 the check could not run.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
# shellcheck source=scripts/dns-leak-check.sh
DNS_LEAK_CHECK_LIBRARY=1 . "$ROOT/scripts/dns-leak-check.sh"
set -euo pipefail

die() {
  printf 'android-dns-leak-check: %s\n' "$1" >&2
  exit 2
}

main() {
  local serial=emulator-5554 baseline=false capture_name capture nonce findings vpn avd_path
  while (($#)); do
    case $1 in
      --serial) [[ $# -ge 2 ]] || die '--serial needs a device'; serial=$2; shift 2 ;;
      --baseline) baseline=true; shift ;;
      *) die "unknown option: $1" ;;
    esac
  done
  for tool in adb tcpdump; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done
  [[ $serial == emulator-* ]] || die 'the capture needs an emulator; use --serial emulator-NNNN'
  adb -s "$serial" get-state >/dev/null 2>&1 || die "$serial is not connected"
  [[ $(adb -s "$serial" shell settings get global wifi_on | tr -d '\r') == 0 ]] \
    || die 'turn Wi-Fi off first (adb shell svc wifi disable): the capture sees only the cellular card'
  vpn=$(adb -s "$serial" shell dumpsys connectivity 2>/dev/null | grep -c 'VPN CONNECTED' || true)
  if $baseline; then
    ((vpn == 0)) || die 'disconnect Ket for the baseline'
  else
    ((vpn > 0)) || die 'no VPN is connected; connect Ket first'
  fi

  nonce="ket-leak-$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
  # The console writes captures under the AVD's console_out directory.
  avd_path=$(adb -s "$serial" emu avd path | head -n 1 | tr -d '\r')
  capture_name="$nonce.pcap"
  capture="$avd_path/console_out/$capture_name"
  adb -s "$serial" emu network capture start "$capture_name" >/dev/null
  sleep 1
  printf 'Probes from %s:\n' "$serial"
  printf '  lookup (A): %s\n' "$nonce-ping.example.com"
  adb -s "$serial" shell ping -c 1 -W 3 "$nonce-ping.example.com" >/dev/null 2>&1 || true
  printf '  lookup (AAAA): %s\n' "$nonce-ping6.example.com"
  adb -s "$serial" shell ping6 -c 1 -W 3 "$nonce-ping6.example.com" >/dev/null 2>&1 || true
  printf '  IPv6 ping: %s\n' "$IPV6_PROBE"
  adb -s "$serial" shell ping6 -c 1 -W 3 "$IPV6_PROBE" >/dev/null 2>&1 || true
  sleep 2
  adb -s "$serial" emu network capture stop >/dev/null
  sleep 1
  [[ -s $capture ]] || die "the emulator wrote no capture to $capture"

  findings=$(tcpdump -n -r "$capture" 2>/dev/null | sed 's/^/emulator /' | leak_findings "$nonce")
  rm -f -- "$capture"
  if [[ -n $findings ]]; then
    printf '\nLEAK: traffic left the device outside the tunnel:\n'
    awk '{ print "  " $0 }' <<<"$findings"
    exit 1
  fi
  printf '\nNo leak: no probe lookup, DNS, or IPv6 traffic left the device outside the tunnel.\n'
}

main "$@"
