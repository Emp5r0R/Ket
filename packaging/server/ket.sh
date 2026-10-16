#!/usr/bin/env bash
# Manage this Ket server. The installer links it as /usr/local/bin/ket:
#
#   sudo ket            show what you can do
#   sudo ket invite     create an invite (asks a few questions)
#   sudo ket doctor     find and explain problems
#
# Each command lives in packaging/server/cli/<topic>.sh.
set -euo pipefail

self=$(readlink -f -- "${BASH_SOURCE[0]}")
source_dir=$(cd -- "$(dirname -- "$self")/../.." && pwd)
install_dir=${KET_INSTALL_DIR:-$source_dir}
for library in paths common releases image units backup update protocols invites keys status doctor uninstall; do
  # shellcheck source=/dev/null
  . "$source_dir/packaging/server/cli/$library.sh"
done

usage() {
  cat <<'USAGE'
Manage this Ket server.

Usage: sudo ket COMMAND [options]

Everyday:
  invite              Create an invite (asks who it is for, devices, days)
  invites             List invites and how many devices use each
  invites show ID     See an invite and the devices connected through it
  invites pause ID    Pause an invite (resume ID turns it back on)
  invites edit ID     Rename, change devices, extend or shorten, or limit
                      which protocols it may use
  revoke ID           Revoke an invite and disconnect its devices
  status              Version, services, connected devices, certificate

When something is wrong:
  doctor              Check everything and explain how to fix problems
  logs [SERVICE]      Show recent logs (add --follow to keep watching)
  restart             Restart Ket, applying changes made to .env

Keeping up to date:
  update              Update to the latest release (--check only reports)
  rollback            Go back to the release before the last update
  backup              Save keys, settings, and grants to /var/backups/ket

Other:
  keys                Show the node fingerprint, or rotate keys
  uninstall           Remove Ket from this server
  version             Print the installed release

Run "sudo ket COMMAND --help" for a command's options.
USAGE
}

cmd_logs() {
  require_root
  require_installation
  local follow=() services=()
  while (($#)); do
    case $1 in
      --follow|-f) follow=(--follow); shift ;;
      -*) fail "unknown option: $1" ;;
      *) services+=("$1"); shift ;;
    esac
  done
  compose logs --no-color --tail=200 "${follow[@]}" "${services[@]}"
}

cmd_restart() {
  require_root
  require_installation
  validate_configuration "$install_dir" || fail '.env fails the configuration check; run: sudo ket doctor'
  compose up --detach --no-build --force-recreate --remove-orphans
  if wait_for_ready; then info 'Ket is running.'; else fail 'Ket did not become ready; run: sudo ket doctor'; fi
}

main() {
  local command=${1:-help}
  (($#)) && shift
  case $command in
    invite) cmd_invite "$@" ;;
    invites) cmd_invites "$@" ;;
    revoke) cmd_revoke "$@" ;;
    status) cmd_status "$@" ;;
    doctor) cmd_doctor "$@" ;;
    logs) cmd_logs "$@" ;;
    restart) cmd_restart "$@" ;;
    update) cmd_update "$@" ;;
    rollback) cmd_rollback "$@" ;;
    backup) cmd_backup "$@" ;;
    keys) cmd_keys "$@" ;;
    uninstall) cmd_uninstall "$@" ;;
    version) installed_ref; printf '\n' ;;
    apply-update) cmd_apply_update "$@" ;;
    help|-h|--help) usage ;;
    *) usage >&2; fail "unknown command: $command" ;;
  esac
}

[[ -n ${KET_CLI_LIBRARY:-} ]] || main "$@"
