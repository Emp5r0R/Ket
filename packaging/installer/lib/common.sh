# shellcheck shell=bash
# Shared helpers for the Ket server installer. Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

fail() {
  printf 'Ket installer error: %s\n' "$1" >&2
  exit 1
}

need_value() {
  [[ $# -ge 2 && -n $2 ]] || fail "$1 requires a value"
}

# version_ge A B: true when version A >= version B (dotted numeric).
version_ge() {
  [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n 1)" == "$2" ]]
}

random_secret() { openssl rand -base64 48 | tr -d '\n'; }

# A 32-byte value as unpadded Base64URL (the node signing key format).
random_base64url_32() { openssl rand 32 | base64 | tr -d '\n=' | tr '+/' '-_'; }

random_hex() { openssl rand -hex "$1"; }

# Converts an unpadded Base64URL 32-byte key (43 characters) to the padded
# standard Base64 form WireGuard expects.
base64url_key_to_standard() {
  local value=${1//-/+}
  value=${value//_//}
  printf '%s=' "$value"
}
