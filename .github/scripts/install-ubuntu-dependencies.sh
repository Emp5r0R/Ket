#!/usr/bin/env bash
set -euo pipefail

[[ $# -gt 0 ]] || {
  printf 'Usage: %s <apt-package>...\n' "$0" >&2
  exit 2
}

if [[ ${KET_APT_ALLOW_UNPRIVILEGED:-0} != 1 && $EUID -ne 0 ]]; then
  printf '%s must run as root.\n' "$0" >&2
  exit 1
fi

source_parts=${KET_APT_SOURCE_PARTS_DIR:-/etc/apt/sources.list.d}
apt_get=${KET_APT_GET:-apt-get}
[[ -d $source_parts ]] || {
  printf 'APT source directory is unavailable: %s\n' "$source_parts" >&2
  exit 1
}
command -v "$apt_get" >/dev/null 2>&1 || {
  printf 'APT command is unavailable: %s\n' "$apt_get" >&2
  exit 1
}

# GitHub runner images include unrelated vendor feeds. A temporary outage in
# one of those feeds must not prevent installing packages from Ubuntu itself.
while IFS= read -r -d '' source; do
  if grep -Fq 'packages.microsoft.com' "$source"; then
    disabled="${source}.ket-disabled"
    [[ ! -e $disabled ]] || {
      printf 'Refusing to overwrite disabled APT source: %s\n' "$disabled" >&2
      exit 1
    }
    mv -- "$source" "$disabled"
    printf 'Disabled unrelated runner APT source: %s\n' "$(basename "$source")"
  fi
done < <(find "$source_parts" -maxdepth 1 -type f \( -name '*.list' -o -name '*.sources' \) -print0)

"$apt_get" -o Acquire::Retries=3 update
DEBIAN_FRONTEND=noninteractive \
  "$apt_get" -o Acquire::Retries=3 install --no-install-recommends -y "$@"
