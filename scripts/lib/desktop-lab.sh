# shellcheck shell=bash
# A disposable Ubuntu 24.04 desktop-like container that runs the shipped
# client code (the connect_check example) under the shipped tunnel service's
# systemd sandbox. Sourced by the DNS leak lab and the roaming lab; never
# executed.
#
# Callers set ROOT and the container variable, and define die; start_client
# sets client_pid for them.
# shellcheck disable=SC2034,SC2154

readonly DESKTOP_LAB_IMAGE=ket-dns-leak-lab:ubuntu-24.04
readonly DESKTOP_LAB_ENGINES=(xray tun2proxy hysteria sslocal wstunnel)

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
  for engine in "${DESKTOP_LAB_ENGINES[@]}"; do
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
  docker build -q -t "$DESKTOP_LAB_IMAGE" - >/dev/null <<'DOCKERFILE'
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

# start_desktop NAME NETWORK ENGINES PROBE [EXTRA_MOUNT...]: starts the
# container and waits for systemd-resolved.
start_desktop() {
  local name=$1 network=$2 engines=$3 probe=$4 attempt
  shift 4
  container=$name
  docker run -d --name "$container" --privileged --cgroupns=private --network "$network" \
    --tmpfs /run --tmpfs /run/lock \
    -v "$engines:/opt/ket/engines:ro" -v "$probe:/opt/ket/connect_check:ro" \
    "$@" "$DESKTOP_LAB_IMAGE" >/dev/null
  for attempt in $(seq 1 30); do
    [[ $(lab systemctl is-active systemd-resolved 2>/dev/null) == active ]] && return 0
    sleep 1
  done
  die "systemd-resolved did not start in the lab container (after $attempt s)"
}

# build_probe: builds this tree's connect_check and prints its path.
build_probe() {
  (cd "$ROOT" && cargo build -q -p ket-client-core --example connect_check)
  printf '%s/debug/examples/connect_check\n' \
    "$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
}

# start_client UNIT LOG PROTOCOL INVITE [FLAG...]: runs connect_check in the
# container under the tunnel service sandbox, in the background of this
# shell; sets client_pid so the caller can wait for it.
start_client() {
  local unit=$1 log=$2 protocol=$3 invite=$4 property
  shift 4
  local -a sandbox=()
  while IFS= read -r property; do sandbox+=(-p "$property"); done < <(unit_sandbox)
  printf '%s\n' "$invite" | docker exec -i "$container" systemd-run --quiet --wait --pipe --collect \
    --unit "$unit" "${sandbox[@]}" \
    /opt/ket/connect_check --engines /opt/ket/engines --runtime /run/ket --protocol "$protocol" "$@" \
    >"$log" 2>&1 &
  client_pid=$!
}

# wait_connected LOG PID SECONDS: true once the client printed `connected`.
wait_connected() {
  local log=$1 client=$2 attempt
  for attempt in $(seq 1 "$3"); do
    grep -q '^connected' "$log" && return 0
    kill -0 "$client" 2>/dev/null || return 1
    sleep 1
  done
  return 1
}
