#!/usr/bin/env bash
# One-command Ket server setup, update, and removal.
#
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- vpn.example.com
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- update
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- uninstall
#
# Downloads the latest Ket release (or KET_REF) and runs that release's own
# installer or updater, so the code that installs a release always matches it.
# Installed servers also have the `ket` command (sudo ket help).
set -euo pipefail

readonly REPOSITORY=Emp5r0R/Ket
readonly INSTALLER_PROTOCOL=2
github_url=${KET_GITHUB_URL:-https://github.com}
codeload_url=${KET_CODELOAD_URL:-https://codeload.github.com}
install_dir=${KET_INSTALL_DIR:-/opt/ket}

fail() {
  printf 'Ket setup error: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: install.sh [install] [DOMAIN] [options]   Set up a new Ket server
       install.sh update                         Update this server to the latest release
       install.sh uninstall [--purge] [--yes]    Remove Ket from this server

Set KET_REF=<tag> to use a specific release. Installer options: see
docs/SERVER_INSTALL.md, or run with "install --help".
EOF
}

command=install
case ${1:-} in
  install) shift ;;
  update | uninstall) command=$1; shift ;;
  help | -h | --help) usage; exit 0 ;;
esac

command -v curl >/dev/null 2>&1 || fail 'curl is required'
command -v tar >/dev/null 2>&1 || fail 'tar is required'

if [[ $command != install ]]; then
  [[ -f $install_dir/.env ]] || fail "no Ket installation in $install_dir (set KET_INSTALL_DIR if it is elsewhere)"
  # Installations with the ket command remove themselves without a download.
  if [[ $command == uninstall && -f $install_dir/packaging/server/ket.sh ]]; then
    exec env "KET_INSTALL_DIR=$install_dir" bash "$install_dir/packaging/server/ket.sh" uninstall "$@"
  fi
fi

ref=${KET_REF:-}
if [[ -z $ref ]]; then
  latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$github_url/$REPOSITORY/releases/latest") \
    || fail 'could not look up the latest Ket release'
  ref=${latest##*/tag/}
fi
[[ $ref =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || fail "unexpected release name: $ref"

work=$(mktemp -d "${TMPDIR:-/tmp}/ket-setup.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
printf 'Downloading Ket %s...\n' "$ref"
curl -fsSL "$codeload_url/$REPOSITORY/tar.gz/$ref" | tar -xzf - -C "$work" --strip-components=1 \
  || fail "could not download Ket $ref"

if [[ $(cat "$work/packaging/installer/protocol" 2>/dev/null) != "$INSTALLER_PROTOCOL" ]]; then
  [[ $command == install ]] || fail "Ket $ref predates the ket command and cannot $command a server; use a newer release"
  cat >&2 <<EOF
Ket $ref was released before one-command setup. Install it with its own installer:

  curl -fsSL https://raw.githubusercontent.com/$REPOSITORY/$ref/packaging/install-server.sh | sudo bash -s -- --mode auto --email you@example.com

Details: https://github.com/$REPOSITORY/blob/$ref/docs/SERVER_INSTALL.md
EOF
  exit 1
fi

case $command in
  uninstall)
    # Older installations are removed by the downloaded release's ket command.
    env "KET_INSTALL_DIR=$install_dir" bash "$work/packaging/server/ket.sh" uninstall "$@"
    ;;
  install)
    # The installer moves the downloaded tree into place.
    trap - EXIT
    exec bash "$work/packaging/install-server.sh" --source-dir "$work" --ref "$ref" "$@"
    ;;
  update)
    # The updater copies the downloaded tree, then removes it.
    trap - EXIT
    exec bash "$work/packaging/server/ket.sh" apply-update \
      --source-dir "$work" --ref "$ref" --install-dir "$install_dir" --remove-source "$@"
    ;;
esac
