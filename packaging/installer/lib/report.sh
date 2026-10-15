# shellcheck shell=bash
# Operator-facing output for the Ket server installer. Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

print_ingress() {
  printf '  TCP 80                 Certificate issuance and renewal\n'
  printf '  TCP 443                Control, REALITY, HTTPS Stealth, WireGuard TLS\n'
  printf '  UDP 443                Hysteria2 and Hysteria XOR\n'
  printf '  TCP 9443               OpenVPN over TLS\n'
  printf '  TCP+UDP %s-%s     Shadowsocks 2022\n' "$shadowsocks_start" "$shadowsocks_end"
}

print_plan() {
  printf '\n=== Ket server plan ===\n'
  case $mode in
    auto) printf 'Address: %s (automatic, no DNS needed)\n' "$domain" ;;
    direct) printf 'Address: %s (your domain, pointing straight at this server)\n' "$domain" ;;
    cloudflare)
      printf 'Address: %s (through Cloudflare)\n' "$domain"
      printf 'Direct address for UDP and raw TLS: %s\n' "$direct_host"
      printf 'Cloudflare: set SSL/TLS to Full (strict) and keep WebSockets on.\n'
      ;;
  esac
  [[ -z $public_ip ]] || printf 'Server IPv4: %s\n' "$public_ip"
  printf '\nOpen these inbound ports in your cloud firewall (source 0.0.0.0/0):\n'
  print_ingress
  printf 'Keep your SSH rule. Never expose TCP 8787, 8445, or 8446.\n'
  printf '=======================\n\n'
}

print_handoff() {
  printf 'Save the invite now; the server never stores it.\n'
  printf '\nNext:\n'
  printf '  1. Install the Ket app: %s\n' "$client_release_url"
  printf '  2. Paste or scan the invite, then connect.\n'
  printf '\nMore invites: cd %s && sudo ./packaging/server/create-invite.sh --devices 2 --days 30\n' "$install_dir"
  printf 'Status:       cd %s && sudo ./packaging/server/compose.sh ps\n' "$install_dir"
  if [[ $wireguard_enabled != true ]]; then
    printf '\nNote: this kernel has no WireGuard support, so WireGuard TLS is off; the other transports work.\n'
  fi
}
