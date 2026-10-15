#!/usr/bin/env bash
# One-command Ket server setup.
#
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash
#   curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- vpn.example.com
#
# Downloads the latest Ket release and runs that release's own installer, so
# the installer always matches the code it installs. Set KET_REF to install a
# specific tag. Any arguments are passed to the installer (see --help).
set -euo pipefail

readonly REPOSITORY=Emp5r0R/Ket
readonly INSTALLER_PROTOCOL=2
github_url=${KET_GITHUB_URL:-https://github.com}
codeload_url=${KET_CODELOAD_URL:-https://codeload.github.com}

fail() {
  printf 'Ket setup error: %s\n' "$1" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail 'curl is required'
command -v tar >/dev/null 2>&1 || fail 'tar is required'

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
  cat >&2 <<EOF
Ket $ref was released before one-command setup. Install it with its own installer:

  curl -fsSL https://raw.githubusercontent.com/$REPOSITORY/$ref/packaging/install-server.sh | sudo bash -s -- --mode auto --email you@example.com

Details: https://github.com/$REPOSITORY/blob/$ref/docs/SERVER_INSTALL.md
EOF
  exit 1
fi

# The installer takes over the downloaded tree and becomes the installation.
trap - EXIT
exec bash "$work/packaging/install-server.sh" --source-dir "$work" --ref "$ref" "$@"
