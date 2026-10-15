# shellcheck shell=bash
# Removing Ket from this server. Sourced by the ket command; never executed.
# shellcheck disable=SC2034,SC2154

uninstall_usage() {
  cat <<'USAGE'
Usage: ket uninstall [--purge] [--yes]

Stops Ket and removes its containers, state, timers, the ket command, and the
installation directory. A final backup of your keys, settings, and grants is
written to /var/backups/ket first, unless --purge is given.

  --purge   Also delete the backups, the Let's Encrypt certificate, and the
            downloaded container images. Nothing of Ket is left behind.
  --yes     Do not ask for confirmation.
USAGE
}

cmd_uninstall() {
  local purge=false yes=false cert_name archive
  while (($#)); do
    case $1 in
      --purge) purge=true; shift ;;
      --yes|-y) yes=true; shift ;;
      -h|--help) uninstall_usage; return 0 ;;
      *) uninstall_usage >&2; fail "unknown option: $1" ;;
    esac
  done
  require_root
  require_installation
  info "This removes Ket $(installed_ref) from $install_dir, including every invite and connected device."
  $purge && info 'With --purge, backups, the certificate, and container images are deleted too.'
  if ! $yes; then
    has_terminal || fail 'no terminal to confirm on; rerun with --yes'
    [[ $(ask "Type 'uninstall' to remove Ket" '') == uninstall ]] || { info 'Nothing changed.'; return 0; }
  fi
  cert_name=$(env_value KET_CERTBOT_NAME)
  if ! $purge; then
    archive=$(create_backup before-uninstall)
    info "Final backup: $archive"
  fi
  compose down --volumes --remove-orphans || warn 'Docker Compose could not remove every container; check: sudo docker ps -a'
  remove_host_integration
  if $purge; then
    remove_images
    remove_certificate "$cert_name"
    rm -rf -- "$(ket_backup_dir)"
  fi
  rm -rf -- "$install_dir" "$install_dir.previous" "$(ket_state_dir)"
  info 'Ket is removed.'
  info 'Firewall rules that the installer added for ports 80, 443, 9443, and the Shadowsocks range are left in place, as other services may use them.'
  $purge || info "To reinstall with the same keys later, keep $(ket_backup_dir)."
}

remove_images() {
  local image
  while read -r image; do
    [[ -z $image ]] || docker_cli rmi --force "$image" >/dev/null 2>&1 || true
  done < <(
    docker_cli image ls --format '{{.Repository}}:{{.Tag}}' "$KET_IMAGE" 2>/dev/null
    docker_cli image ls --format '{{.Repository}}:{{.Tag}}' "$KET_PUBLISHED_IMAGE" 2>/dev/null
  )
}

remove_certificate() {
  [[ -n $1 && $1 =~ ^[A-Za-z0-9.-]+$ ]] || return 0
  local letsencrypt=${KET_LETSENCRYPT_DIR:-/etc/letsencrypt}
  rm -rf -- "$letsencrypt/live/$1" "$letsencrypt/archive/$1" "$letsencrypt/renewal/$1.conf"
}
