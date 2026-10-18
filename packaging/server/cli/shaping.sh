# shellcheck shell=bash
# Optional fair sharing of this server's upload to its devices: CAKE on the
# interface that carries the default route, at a rate set just below what the
# server's plan or network allows. Off until an admin sets a rate. Sourced by
# the ket command; never executed.
#
# Without a rate below the real limit, the queue forms at the provider and
# CAKE never sees it. CAKE then shares the rate by destination address
# (dual-dsthost), so one device's many downloads cannot crowd out another's.
# It cannot tell apart devices that reach Ket through Cloudflare or share one
# public address, and it does not shape what devices upload.
# shellcheck disable=SC2034,SC2154

shaping_tc() { "${KET_TC:-tc}" "$@"; }
shaping_ip() { "${KET_IP:-ip}" "$@"; }

shaping_usage() {
  cat <<'USAGE'
Usage: sudo ket shaping [COMMAND]

  (nothing)        Show whether fair sharing is on, at what rate, and where
  set RATE         Turn it on at RATE, such as 950mbit or 2gbit
  off              Turn it off

Fair sharing keeps one device's downloads from crowding out the others' when
the server's connection is full. Set RATE a little below the speed the server's
provider sells (for example 950mbit on a 1 Gbit/s plan); a higher rate does
nothing. It shares what the server sends to devices that connect to it
directly, not uploads, and not between devices that reach it through
Cloudflare or share one public address.
USAGE
}

cmd_shaping() {
  local command=${1:-status}
  (($#)) && shift
  case $command in
    status) shaping_status ;;
    set) shaping_set "$@" ;;
    off) shaping_off ;;
    apply) shaping_apply ;;
    -h|--help|help) shaping_usage ;;
    *) shaping_usage >&2; fail "unknown shaping command: $command" ;;
  esac
}

# shaping_rate TEXT: prints TEXT as tc's rate (950mbit, 2gbit), or fails.
shaping_rate() {
  local rate=${1,,}
  [[ $rate =~ ^([1-9][0-9]{0,5})(mbit|gbit)$ ]] || return 1
  [[ ${BASH_REMATCH[2]} == gbit ]] && ((10#${BASH_REMATCH[1]} > 400)) && return 1
  printf '%s' "$rate"
}

# shaping_interface: the interface that carries the IPv4 default route.
shaping_interface() {
  local interface
  interface=$(shaping_ip -o -4 route show default 2>/dev/null \
    | awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }')
  [[ $interface =~ ^[A-Za-z0-9._@-]{1,15}$ ]] || return 1
  printf '%s' "${interface%%@*}"
}

# shaping_active INTERFACE: true when CAKE is the interface's root queue.
shaping_active() {
  shaping_tc qdisc show dev "$1" root 2>/dev/null | grep -q '^qdisc cake '
}

# Applies the saved rate, or does nothing when none is set. The boot unit
# runs this, so it must stay quiet and safe to repeat.
shaping_apply() {
  local rate interface
  rate=$(env_value KET_EGRESS_RATE)
  [[ -n $rate ]] || return 0
  rate=$(shaping_rate "$rate") || fail "KET_EGRESS_RATE in .env is not a rate such as 950mbit"
  interface=$(shaping_interface) || fail 'could not find the interface of the default route'
  shaping_tc qdisc replace dev "$interface" root cake bandwidth "$rate" besteffort dual-dsthost \
    || fail "could not start CAKE on $interface; this server's kernel may lack sch_cake"
}

shaping_status() {
  require_root
  require_installation
  local rate interface
  rate=$(env_value KET_EGRESS_RATE)
  if [[ -z $rate ]]; then
    info 'Fair sharing is off. Turn it on with: sudo ket shaping set 950mbit'
    info '(use a rate a little below your server plan; see: sudo ket shaping --help)'
    return 0
  fi
  if ! interface=$(shaping_interface); then
    warn 'fair sharing is set but the server has no default route'
    return 1
  fi
  if shaping_active "$interface"; then
    info "Fair sharing is on at $rate on $interface."
  else
    warn "fair sharing is set to $rate but not running on $interface; start it with: sudo ket shaping set $rate"
    return 1
  fi
}

shaping_set() {
  require_root
  require_installation
  local rate
  [[ -n ${1:-} ]] || fail 'give a rate, such as: sudo ket shaping set 950mbit'
  rate=$(shaping_rate "$1") || fail "\"$1\" is not a rate; use a number with mbit or gbit, such as 950mbit"
  set_env_value KET_EGRESS_RATE "$rate"
  shaping_apply
  install_shaping_unit
  info "Fair sharing is on at $rate on $(shaping_interface)."
  info 'Ket turns it on again after a restart. Turn it off with: sudo ket shaping off'
}

shaping_off() {
  require_root
  require_installation
  local interface
  set_env_value KET_EGRESS_RATE ''
  remove_shaping_unit
  if interface=$(shaping_interface) && shaping_active "$interface"; then
    shaping_tc qdisc del dev "$interface" root || fail "could not remove CAKE from $interface"
  fi
  info 'Fair sharing is off.'
}

# The unit applies the saved rate at boot, once the network is up.
install_shaping_unit() {
  cat > "$(ket_systemd_dir)/ket-shaping.service" <<UNIT
[Unit]
Description=Share the Ket server's upload fairly between devices (CAKE)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=$install_dir/packaging/server/ket.sh shaping apply

[Install]
WantedBy=multi-user.target
UNIT
  systemd_is_simulated && return 0
  systemctl daemon-reload
  systemctl enable ket-shaping.service >/dev/null
}

remove_shaping_unit() {
  local unit
  unit="$(ket_systemd_dir)/ket-shaping.service"
  [[ -e $unit ]] || return 0
  systemd_is_simulated || systemctl disable ket-shaping.service >/dev/null 2>&1 || true
  rm -f "$unit"
  systemd_is_simulated || systemctl daemon-reload
}
