#!/usr/bin/env bash
# Tests the DNS leak check's verdicts on captured traffic. The capture and the
# lookups themselves run against a real connection (scripts/dns-leak-lab.sh).
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=/dev/null
DNS_LEAK_CHECK_LIBRARY=1 . "$root/scripts/dns-leak-check.sh"
set -euo pipefail

fail_test() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

readonly NONCE=ket-leak-0123456789ab
readonly SERVER=203.0.113.7

# expect_leak LINE REASON: the line is reported, and for this reason.
expect_leak() {
  local findings
  findings=$(leak_findings "$NONCE" "$SERVER" 2001:db8::7 <<<"$1")
  [[ $findings == "$2"* ]] || fail_test "expected [$2] for: $1 (got: ${findings:-nothing})"
}

expect_clean() {
  local findings
  findings=$(leak_findings "$NONCE" "$SERVER" 2001:db8::7 <<<"$1")
  [[ -z $findings ]] || fail_test "reported a leak for: $1 ($findings)"
}

expect_leak "eth0 12:00:00.1 IP 192.168.1.20.41000 > 192.168.1.1.53: 4660+ A? $NONCE-glibc.example.com. (45)" \
  'probe lookup left through eth0'
expect_leak "wlan0 12:00:00.1 IP6 fe80::2.5353 > ff02::fb.5353: 0 A (QM)? $NONCE-glibc.example.com. (45)" \
  'probe lookup left through wlan0'
expect_leak "eth0 12:00:00.1 IP 172.17.0.2.40000 > $SERVER.53: 1+ A? $NONCE-dig.example.com. (40)" \
  'probe lookup left through eth0'
expect_leak 'eth0 12:00:00.1 IP 192.168.1.20.41000 > 1.1.1.1.53: 77+ A? connectivity-check.ubuntu.com. (47)' \
  'DNS outside the tunnel on eth0'
expect_leak 'eth0 12:00:00.1 IP 1.1.1.1.53 > 192.168.1.20.41000: 77 1/0/0 A 185.125.190.17 (63)' \
  'DNS outside the tunnel on eth0'
expect_leak 'eth0 12:00:00.1 IP 192.168.1.20.55000 > 9.9.9.9.853: Flags [S], seq 1, length 0' \
  'DNS outside the tunnel on eth0'
expect_leak 'eth0 12:00:00.1 IP6 fe80::2.41000 > fe80::1.53: 9+ AAAA? example.org. (29)' \
  'DNS outside the tunnel on eth0'
expect_leak 'eth0 12:00:00.1 IP6 2001:db8::20.41000 > 2606:4700:4700::1111.443: Flags [S], seq 1, length 0' \
  'IPv6 outside the tunnel on eth0'

expect_clean "eth0 12:00:00.1 IP 172.17.0.2.40000 > $SERVER.443: Flags [P.], seq 1:100, length 99"
expect_clean 'eth0 12:00:00.1 IP6 2001:db8::20.40000 > 2001:db8::7.443: Flags [S], seq 1, length 0'
expect_clean 'eth0 12:00:00.1 IP 192.168.1.20.5353 > 224.0.0.251.5353: 0 PTR (QM)? _ipp._tcp.local. (32)'
expect_clean 'eth0 12:00:00.1 IP 10.0.0.53 > 10.0.0.1: ICMP echo request, id 1, seq 1, length 64'
expect_clean 'eth0 12:00:00.1 IP 192.168.1.20.1853 > 192.168.1.9.8853: UDP, length 20'
expect_clean 'eth0 12:00:00.1 IP6 fe80::2 > ff02::2: ICMP6, router solicitation, length 16'
expect_clean 'eth0 12:00:00.1 IP6 fe80::2 > fe80::1: ICMP6, neighbor solicitation, who has fe80::1, length 32'
expect_clean 'eth0 12:00:00.1 ARP, Request who-has 192.168.1.1 tell 192.168.1.20, length 28'
expect_clean 'eth0 12:00:00.1 IP 172.17.0.2.40000 > 203.0.113.70.443: Flags [S], seq 1, length 0'
# A server allowance must not cover a neighbouring address.
[[ -n $(leak_findings "$NONCE" 203.0.113.7 <<<'eth0 1 IP 172.17.0.2.40000 > 203.0.113.70.53: 1+ A? a.test. (20)') ]] \
  || fail_test 'an allowance for 203.0.113.7 covered 203.0.113.70'

printf 'DNS leak check tests passed.\n'
