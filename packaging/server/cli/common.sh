# shellcheck shell=bash
# Shared helpers for the ket server command. Sourced, never executed.
# Globals (install_dir, source_dir) are set by ket.sh.
# shellcheck disable=SC2034,SC2154

readonly KET_REPOSITORY=Emp5r0R/Ket
readonly KET_IMAGE=ket-control-plane

fail() {
  printf 'ket: %s\n' "$1" >&2
  exit 1
}

info() { printf '%s\n' "$*"; }
warn() { printf 'Warning: %s\n' "$*" >&2; }

require_root() {
  [[ $EUID -eq 0 ]] || fail 'run this as root, for example: sudo ket ...'
}

require_commands() {
  local name
  for name in "$@"; do
    command -v "$name" >/dev/null 2>&1 || fail "$name is required"
  done
}

require_installation() {
  [[ -f $install_dir/.env ]] || fail "no Ket installation found in $install_dir"
}

# env_value KEY [FILE]: the last value of KEY in an env file, unquoted.
env_value() {
  local value
  value=$(sed -n "s/^$1=//p" "${2:-$install_dir/.env}" 2>/dev/null | tail -n 1)
  value=${value#\"}
  printf '%s' "${value%\"}"
}

# control_url PATH [DIR]: the control plane's loopback URL for PATH, on the
# host port the installation publishes it on (KET_CONTROL_PORT, default 8787).
control_url() {
  local port
  port=$(env_value KET_CONTROL_PORT "${2:-$install_dir}/.env")
  printf 'http://127.0.0.1:%s%s' "${port:-8787}" "$1"
}

# set_env_value KEY VALUE [FILE]: replaces or appends KEY, keeping mode 0600.
set_env_value() {
  local key=$1 value=$2 file=${3:-$install_dir/.env} next
  [[ $value != *$'\n'* ]] || fail "refusing a multi-line value for $key"
  next=$(mktemp "$file.XXXXXX")
  awk -v key="$key" -v value="$value" '
    BEGIN { written = 0 }
    index($0, key "=") == 1 { if (!written) { print key "=" value; written = 1 }; next }
    { print }
    END { if (!written) print key "=" value }
  ' "$file" > "$next"
  chmod 0600 "$next"
  mv -f "$next" "$file"
}

# validate_configuration DIR: runs the release's .env preflight in DIR.
validate_configuration() {
  # shellcheck source=/dev/null
  (cd "$1" && set -a && . ./.env && set +a && ./packaging/validate-env.sh >/dev/null 2>&1)
}

compose_in() {
  local dir=$1
  shift
  "$dir/packaging/server/compose.sh" "$@"
}

compose() { compose_in "$install_dir" "$@"; }

docker_cli() { "${KET_DOCKER:-docker}" "$@"; }

# Interactive input comes from the terminal even when stdin is a pipe.
has_terminal() { [[ -r $(ket_tty) && -w $(ket_tty) ]] && { : < "$(ket_tty)"; } 2>/dev/null; }

# ask PROMPT DEFAULT: prints the answer (the default for an empty answer).
ask() {
  local prompt=$1 default=${2:-} answer
  printf '%s%s: ' "$prompt" "${default:+ [$default]}" >> "$(ket_tty)"
  IFS= read -r answer < "$(ket_tty)" || answer=''
  printf '%s' "${answer:-$default}"
}

# confirm PROMPT: true only for an explicit yes.
confirm() {
  local answer
  answer=$(ask "$1 (yes/no)" no)
  [[ ${answer,,} == y || ${answer,,} == yes ]]
}

# The release this installation runs: a tag such as v1.1.0, "source" for a
# checkout install, or "unknown".
installed_ref() {
  local dir=${1:-$install_dir} ref
  if [[ -r $dir/.ket-release ]]; then
    ref=$(head -n 1 "$dir/.ket-release")
  elif [[ -d $dir/.git ]] && command -v git >/dev/null 2>&1; then
    ref=$(git -c safe.directory="$dir" -C "$dir" describe --tags --exact-match 2>/dev/null || true)
  fi
  [[ ${ref:-} =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$ ]] || ref=unknown
  printf '%s' "$ref"
}

# ref_version REF: the MAJOR.MINOR.PATCH of a vX.Y.Z tag, or nothing.
ref_version() {
  local version=${1#v}
  [[ $1 == v* && $version =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] \
    && printf '%s' "$version"
}

# public_host: the host name from KET_PUBLIC_URL.
public_host() {
  local url
  url=$(env_value KET_PUBLIC_URL)
  url=${url#https://}
  url=${url#http://}
  url=${url%%/*}
  printf '%s' "${url%%:*}"
}

timestamp() { date -u +%Y%m%dT%H%M%SZ; }
