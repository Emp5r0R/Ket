# shellcheck shell=bash
# Troubleshooting: checks the host, configuration, services, network, and
# certificate, and says what to do about each problem. Sourced by the ket
# command; never executed.
# shellcheck disable=SC2034,SC2154

doctor_failures=0
doctor_warnings=0

pass() { printf '  ok    %s\n' "$1"; }
attention() {
  printf '  WARN  %s\n' "$1"
  [[ -z ${2:-} ]] || printf '        -> %s\n' "$2"
  doctor_warnings=$((doctor_warnings + 1))
}
problem() {
  printf '  FAIL  %s\n' "$1"
  [[ -z ${2:-} ]] || printf '        -> %s\n' "$2"
  doctor_failures=$((doctor_failures + 1))
}

listening() {
  # listening tcp|udp PORT
  local flag=-lnt
  [[ $1 == udp ]] && flag=-lnu
  ss -H "$flag" "sport = :$2" 2>/dev/null | grep -q .
}

enabled_flag() { [[ $(env_value "$1") == true ]]; }

doctor_host() {
  printf 'Host\n'
  if docker_cli info >/dev/null 2>&1; then pass 'Docker is running'; else
    problem 'Docker is not running' 'sudo systemctl start docker'
    return
  fi
  if docker_cli compose version >/dev/null 2>&1; then pass 'Docker Compose v2 is available'; else
    problem 'Docker Compose v2 is missing' 'install the docker-compose-plugin package'
  fi
  local free_kib
  free_kib=$(df -Pk "$install_dir" | awk 'NR == 2 { print $4 }')
  if ((free_kib < 1048576)); then
    attention "only $((free_kib / 1024)) MiB free disk space" 'free space, or run: sudo docker image prune'
  else
    pass 'enough free disk space'
  fi
  if [[ $(timedatectl show -p NTPSynchronized --value 2>/dev/null) == yes ]]; then pass 'the clock is synchronized'; else
    attention 'the clock may not be synchronized; TLS and invite expiry depend on it' 'sudo timedatectl set-ntp true'
  fi
  if enabled_flag KET_WIREGUARD_ENABLED; then
    if [[ -d /sys/module/wireguard ]] || modprobe -n wireguard >/dev/null 2>&1; then pass 'the kernel supports WireGuard'; else
      problem 'WireGuard TLS is on, but the kernel has no WireGuard support' 'set KET_WIREGUARD_ENABLED=false in .env, then: sudo ket restart'
    fi
  fi
  local rate interface
  rate=$(env_value KET_EGRESS_RATE)
  if [[ -n $rate ]]; then
    if interface=$(shaping_interface) && shaping_active "$interface"; then
      pass "fair sharing is on at $rate on $interface"
    else
      attention "fair sharing is set to $rate but is not running" "sudo ket shaping set $rate"
    fi
  fi
}

doctor_configuration() {
  printf 'Configuration\n'
  if validate_configuration "$install_dir"; then
    pass '.env passes the configuration check'
  else
    problem '.env fails the configuration check' "cd $install_dir && sudo bash -c 'set -a; . ./.env; ./packaging/validate-env.sh'"
  fi
  if [[ -n $(env_value KET_NODE_SIGNING_KEY) ]]; then pass 'a node signing key is set, so invites work'; else
    attention 'no node signing key: the server issues access codes but no invites' 'sudo ket keys rotate-node-key'
  fi
}

doctor_services() {
  printf 'Services\n'
  local service state health any=false
  while read -r service state health; do
    any=true
    if [[ $state != running ]]; then
      problem "$service is $state" "sudo ket logs $service"
    elif [[ $health == unhealthy ]]; then
      problem "$service is unhealthy" "sudo ket logs $service"
    else
      pass "$service is running"
    fi
  done < <(service_states)
  $any || problem 'no Ket services exist' 'sudo ket restart'
  if curl --fail --silent --max-time 5 "$(control_url /readyz)" >/dev/null 2>&1; then
    pass 'the control plane is ready'
  else
    problem 'the control plane is not ready' 'sudo ket logs control-plane'
  fi
  local host
  host=$(public_host)
  if curl --fail --silent --max-time 5 --resolve "$host:443:127.0.0.1" "https://$host/" >/dev/null 2>&1; then
    pass "the HTTPS edge answers for $host"
  else
    problem "the HTTPS edge does not answer for $host" 'sudo ket logs edge-proxy'
  fi
}

doctor_network() {
  printf 'Network\n'
  local host direct addresses
  host=$(public_host)
  direct=$(env_value KET_XRAY_PUBLIC_HOST)
  for name in "$host" ${direct:+"$direct"}; do
    addresses=$(getent ahostsv4 "$name" 2>/dev/null | awk '{ print $1 }' | sort -u | tr '\n' ' ')
    if [[ -n $addresses ]]; then pass "$name resolves to ${addresses% }"; else
      problem "$name does not resolve" 'check its DNS A record'
    fi
  done
  if listening tcp 443; then pass 'TCP 443 is listening'; else problem 'nothing listens on TCP 443' 'sudo ket restart'; fi
  if enabled_flag KET_HYSTERIA_ENABLED; then
    if listening udp 443; then pass 'UDP 443 is listening'; else problem 'nothing listens on UDP 443 (Hysteria2)' 'sudo ket logs hysteria2'; fi
  fi
  if enabled_flag KET_OPENVPN_ENABLED; then
    if listening tcp "$(env_value KET_OPENVPN_PUBLIC_PORT)"; then pass "TCP $(env_value KET_OPENVPN_PUBLIC_PORT) is listening"; else
      problem "nothing listens on TCP $(env_value KET_OPENVPN_PUBLIC_PORT) (OpenVPN)" 'sudo ket logs openvpn-stunnel'
    fi
  fi
  printf '        Listening here does not prove your cloud firewall lets traffic in;\n'
  printf '        compare its rules with the ports in docs/SERVER_INSTALL.md.\n'
}

cloudflared_cli() { "${KET_CLOUDFLARED:-cloudflared}" "$@"; }

# tunnel_bypass_service: prints the Cloudflare Tunnel service that receives
# https://<public host>/metrics when it is the control plane itself, which
# skips the Ket edge and makes the operational endpoints public. Fails when
# there is no tunnel configuration or it routes the request elsewhere.
tunnel_bypass_service() {
  local config=${KET_CLOUDFLARED_CONFIG:-/etc/cloudflared/config.yml} service port
  [[ -r $config ]] || return 1
  service=$(cloudflared_cli --config "$config" tunnel ingress rule "https://$(public_host)/metrics" 2>/dev/null \
    | sed -n 's/^[[:space:]]*service:[[:space:]]*//p' | head -n 1)
  port=$(env_value KET_CONTROL_PORT)
  [[ $service =~ ^https?://(127\.0\.0\.1|localhost|\[::1\]):${port:-8787}(/.*)?$ ]] || return 1
  printf '%s' "$service"
}

doctor_tunnel() {
  local config=${KET_CLOUDFLARED_CONFIG:-/etc/cloudflared/config.yml} host service
  [[ -r $config ]] && command -v "${KET_CLOUDFLARED:-cloudflared}" >/dev/null 2>&1 || return 0
  printf 'Cloudflare Tunnel\n'
  host=$(public_host)
  if service=$(tunnel_bypass_service); then
    problem "the tunnel sends $host to the control plane ($service), past the Ket edge, so /metrics and other operational endpoints are public" \
      "route $host to the edge instead: service https://127.0.0.1:443 with originRequest.originServerName $host (docs/XHTTP_STEALTH.md#route-cloudflare-tunnel), then: sudo systemctl restart cloudflared"
  else
    pass "the tunnel keeps $host's operational endpoints private"
  fi
}

doctor_certificate() {
  printf 'Certificate\n'
  local days
  if days=$(certificate_days_left "$install_dir/secrets/tls/fullchain.pem"); then
    if ((days < 3)); then
      problem "the TLS certificate expires in $days days" 'sudo systemctl start ket-certificates.service'
    elif ((days < 14)); then
      attention "the TLS certificate expires in $days days" 'check renewal: sudo systemctl status ket-certificates.service'
    else
      pass "the TLS certificate is valid for $days more days"
    fi
  else
    problem 'the TLS certificate is missing' 'sudo systemctl start ket-certificates.service'
  fi
  if uses_host_certbot; then
    pass 'renewal runs through the host certbot (v1.0.x installation)'
  elif systemd_is_simulated || systemctl is-active --quiet ket-certificates.timer; then
    pass 'the renewal timer is active'
  else
    problem 'the renewal timer is not active' 'sudo systemctl enable --now ket-certificates.timer'
  fi
  if systemd_is_simulated || systemctl is-active --quiet ket-update.timer; then
    pass 'the daily update check is active'
  else
    attention 'the daily update check is not active' 'sudo systemctl enable --now ket-update.timer'
  fi
}

cmd_doctor() {
  require_root
  require_installation
  require_commands curl jq openssl ss getent
  info "Checking Ket $(installed_ref) in $install_dir"
  doctor_host
  doctor_configuration
  doctor_services
  doctor_network
  doctor_tunnel
  doctor_certificate
  info ''
  if ((doctor_failures > 0)); then
    info "$doctor_failures problem(s) and $doctor_warnings warning(s) found."
    return 1
  fi
  if ((doctor_warnings > 0)); then
    info "No problems found; $doctor_warnings warning(s)."
  else
    info 'No problems found.'
  fi
}
