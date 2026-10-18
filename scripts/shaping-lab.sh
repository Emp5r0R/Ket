#!/usr/bin/env bash
# Fair-sharing lab: checks on a real kernel that "ket shaping" shares a full
# server connection between devices.
#
#   scripts/shaping-lab.sh [--seconds N]
#
# A server container sends to two device containers through a router
# container whose single 60 Mbit/s queue stands in for the provider's limit.
# Device A pulls with eight connections and device B with one, at the same
# time, three times:
#   1. without fair sharing (the control): A must take most of the link, or
#      the lab cannot show anything;
#   2. after "ket shaping set 50mbit" on the server (the real ket.sh): B must get close to half, and the
#      two together close to 50 Mbit/s;
#   3. after "ket shaping off": CAKE must be gone again.
# Needs Docker and a kernel with sch_cake. Nothing outside the containers is
# changed.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
readonly IMAGE=ket-shaping-lab:ubuntu-24.04
readonly UPLINK=ket-shaping-uplink DOWNLINK=ket-shaping-downlink
readonly SERVER=ket-shaping-server ROUTER=ket-shaping-router
readonly DEVICE_A=ket-shaping-device-a DEVICE_B=ket-shaping-device-b
readonly PROVIDER_RATE=60mbit SHAPING_RATE=50mbit SHAPING_MBPS=50
seconds=15

fail() {
  printf 'shaping-lab: %s\n' "$1" >&2
  exit 1
}

while (($#)); do
  case $1 in
    --seconds) [[ ${2:-} =~ ^[1-9][0-9]?$ ]] || fail '--seconds needs 1-99'; seconds=$2; shift 2 ;;
    -h|--help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
done

cleanup() {
  docker rm -f "$SERVER" "$ROUTER" "$DEVICE_A" "$DEVICE_B" >/dev/null 2>&1 || true
  docker network rm "$UPLINK" "$DOWNLINK" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

build_image() {
  local attempt
  for attempt in 1 2 3; do
    docker build -q -t "$IMAGE" - >/dev/null <<'DOCKERFILE' && return 0
FROM ubuntu:24.04
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends iperf3 iproute2 jq procps \
 && rm -rf /var/lib/apt/lists/*
DOCKERFILE
    printf 'shaping-lab: image build failed (attempt %s), retrying\n' "$attempt" >&2
    sleep 5
  done
  fail 'could not build the lab image'
}

on() { docker exec "$1" "${@:2}"; }

# start NAME NETWORK ADDRESS [DOCKER-OPTION...]
start() {
  local name=$1 network=$2 address=$3
  docker run -d --name "$name" --cap-add NET_ADMIN --network "$network" --ip "$address" \
    "${@:4}" -v "$ROOT:/ket:ro" "$IMAGE" sleep infinity >/dev/null
}

build_image
docker network create --subnet 172.30.1.0/24 "$UPLINK" >/dev/null
docker network create --subnet 172.30.2.0/24 "$DOWNLINK" >/dev/null
start "$SERVER" "$UPLINK" 172.30.1.10
start "$ROUTER" "$UPLINK" 172.30.1.2 --sysctl net.ipv4.ip_forward=1
docker network connect --ip 172.30.2.2 "$DOWNLINK" "$ROUTER"
start "$DEVICE_A" "$DOWNLINK" 172.30.2.11
start "$DEVICE_B" "$DOWNLINK" 172.30.2.12

# The server reaches the devices, and they reach it, through the router,
# whose queue toward the devices is the provider's: one FIFO at 60 Mbit/s.
on "$SERVER" ip route add 172.30.2.0/24 via 172.30.1.2
for device in "$DEVICE_A" "$DEVICE_B"; do
  on "$device" ip route add 172.30.1.0/24 via 172.30.2.2
done
router_downlink=$(on "$ROUTER" ip -o -4 addr show | awk '$4 ~ /^172\.30\.2\./ { print $2 }')
router_downlink=${router_downlink%%@*}
on "$ROUTER" tc qdisc replace dev "$router_downlink" root tbf rate "$PROVIDER_RATE" burst 32kb latency 300ms

# The server is a Ket installation as far as the ket command can tell.
on "$SERVER" sh -c 'mkdir -p /opt/ket /tmp/systemd && printf "KET_ADMIN_TOKEN=lab\n" > /opt/ket/.env'
ket() {
  on "$SERVER" env KET_INSTALL_DIR=/opt/ket KET_SYSTEMD_DIR=/tmp/systemd bash /ket/packaging/server/ket.sh "$@"
}
on "$SERVER" sh -c 'iperf3 -s -D -p 5201 && iperf3 -s -D -p 5202'
sleep 1

# measure: what A (8 connections) and B (1) each received, in Mbit/s, while
# both pull from the server at the same time.
measure() {
  on "$DEVICE_A" sh -c "iperf3 -c 172.30.1.10 -p 5201 -R -P 8 -t $seconds -J > /tmp/a.json" &
  local a=$!
  on "$DEVICE_B" sh -c "iperf3 -c 172.30.1.10 -p 5202 -R -P 1 -t $seconds -J > /tmp/b.json" &
  local b=$!
  wait "$a" "$b" || fail 'an iperf3 run failed'
  a_mbps=$(on "$DEVICE_A" jq '.end.sum_received.bits_per_second / 1e6 | floor' /tmp/a.json)
  b_mbps=$(on "$DEVICE_B" jq '.end.sum_received.bits_per_second / 1e6 | floor' /tmp/b.json)
  printf '  device A (8 connections): %s Mbit/s, device B (1 connection): %s Mbit/s\n' "$a_mbps" "$b_mbps"
}

printf '1. Without fair sharing (the provider queue alone):\n'
measure
((a_mbps > 3 * b_mbps)) \
  || fail "control: device A did not crowd out device B ($a_mbps vs $b_mbps), so this lab cannot show fairness"

printf '2. After: ket shaping set %s\n' "$SHAPING_RATE"
ket shaping set "$SHAPING_RATE" | sed 's/^/  /'
on "$SERVER" tc qdisc show dev eth0 root | grep -q "^qdisc cake .*bandwidth 50Mbit" \
  || fail 'CAKE is not the server interface queue at 50 Mbit/s'
on "$SERVER" tc qdisc show dev eth0 root | grep -q 'dual-dsthost' || fail 'CAKE does not share by destination'
measure
total=$((a_mbps + b_mbps))
((b_mbps * 100 >= total * 40)) || fail "device B got $b_mbps of $total Mbit/s; fair sharing should give it close to half"
((total * 100 >= SHAPING_MBPS * 80)) || fail "only $total Mbit/s got through; fair sharing should use most of $SHAPING_RATE"
ket shaping | sed 's/^/  /'

printf '3. After: ket shaping off\n'
ket shaping off | sed 's/^/  /'
if on "$SERVER" tc qdisc show dev eth0 root | grep -q '^qdisc cake '; then fail 'CAKE was left on the interface'; fi

printf 'Fair sharing gave a one-connection device %s of %s Mbit/s next to an eight-connection one, and turned off cleanly.\n' \
  "$b_mbps" "$total"
