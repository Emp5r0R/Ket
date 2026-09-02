#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
installer="$root/.github/scripts/install-ubuntu-dependencies.sh"
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
sources="$scratch/sources"
mkdir "$sources"
printf 'Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\n' > "$sources/ubuntu.sources"
printf 'deb https://packages.microsoft.com/repos/azure-cli noble main\n' > "$sources/azure-cli.list"
printf 'Types: deb\nURIs: https://packages.microsoft.com/ubuntu/24.04/prod\n' > "$sources/microsoft.sources"
printf 'ignored\n' > "$sources/vendor.txt"

fake_apt="$scratch/apt-get"
cat > "$fake_apt" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'frontend=%s args=%s\n' "${DEBIAN_FRONTEND:-unset}" "$*" >> "$KET_APT_LOG"
EOF
chmod 0700 "$fake_apt"

export KET_APT_ALLOW_UNPRIVILEGED=1
export KET_APT_SOURCE_PARTS_DIR="$sources"
export KET_APT_GET="$fake_apt"
export KET_APT_LOG="$scratch/apt.log"
env -u DEBIAN_FRONTEND "$installer" package-one package-two

[[ -f $sources/ubuntu.sources && -f $sources/vendor.txt ]]
[[ ! -e $sources/azure-cli.list && -f $sources/azure-cli.list.ket-disabled ]]
[[ ! -e $sources/microsoft.sources && -f $sources/microsoft.sources.ket-disabled ]]
grep -Fxq 'frontend=unset args=-o Acquire::Retries=3 update' "$KET_APT_LOG"
grep -Fxq \
  'frontend=noninteractive args=-o Acquire::Retries=3 install --no-install-recommends -y package-one package-two' \
  "$KET_APT_LOG"
[[ $(wc -l < "$KET_APT_LOG") -eq 2 ]]

if "$installer" >/dev/null 2>&1; then
  printf 'Installer accepted an empty package list.\n' >&2
  exit 1
fi

printf 'Ubuntu dependency installer tests passed.\n'
