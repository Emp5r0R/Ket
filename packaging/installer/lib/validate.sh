# shellcheck shell=bash
# Input validation for the Ket server installer. Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

valid_hostname() {
  local hostname=$1 label
  local IFS=.
  local -a labels
  [[ ${#hostname} -le 253 && $hostname == *.* && $hostname != *..* \
    && $hostname != .* && $hostname != *. ]] || return 1
  read -r -a labels <<<"$hostname"
  for label in "${labels[@]}"; do
    [[ ${#label} -ge 1 && ${#label} -le 63 ]] || return 1
    [[ $label =~ ^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$ ]] || return 1
  done
}

valid_email() {
  [[ $1 =~ ^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$ ]]
}

valid_display_text() {
  [[ -n $1 && ${#1} -le 64 ]] || return 1
  [[ $1 != [[:space:]]* && $1 != *[[:space:]] ]] || return 1
  [[ ! $1 =~ [[:cntrl:]] ]] || return 1
  [[ $1 != *'"'* && $1 != *'$'* && $1 != *'`'* ]] || return 1
  [[ $1 != *\\* ]]
}

valid_public_ipv4() {
  local value=$1 octet
  local IFS=.
  local -a octets
  [[ $value =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1
  read -r -a octets <<<"$value"
  [[ ${#octets[@]} -eq 4 ]] || return 1
  for octet in "${octets[@]}"; do
    [[ $octet =~ ^(0|[1-9][0-9]{0,2})$ ]] || return 1
    ((octet <= 255)) || return 1
  done
  ((octets[0] != 0 && octets[0] != 10 && octets[0] != 127 && octets[0] < 224)) || return 1
  ((!(octets[0] == 100 && octets[1] >= 64 && octets[1] <= 127))) || return 1
  ((!(octets[0] == 169 && octets[1] == 254))) || return 1
  ((!(octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31))) || return 1
  ((!(octets[0] == 192 && (octets[1] == 168 || (octets[1] == 0 && (octets[2] == 0 || octets[2] == 2)))))) || return 1
  ((!(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19 || (octets[1] == 51 && octets[2] == 100))))) || return 1
  ((!(octets[0] == 203 && octets[1] == 0 && octets[2] == 113))) || return 1
}

# Validates the global location fields set by detection or by options.
validate_location() {
  [[ $country_code =~ ^[A-Z]{2}$ ]] || fail '--country-code must contain two letters'
  valid_display_text "$country_name" || fail '--country-name must be 1-64 safe printable characters'
  valid_display_text "$city" || fail '--city must be 1-64 safe printable characters'
  [[ $latitude =~ ^-?[0-9]+([.][0-9]+)?$ ]] || fail '--latitude must be numeric'
  [[ $longitude =~ ^-?[0-9]+([.][0-9]+)?$ ]] || fail '--longitude must be numeric'
  awk -v value="$latitude" 'BEGIN { exit !(value >= -90 && value <= 90) }' \
    || fail '--latitude must be between -90 and 90'
  awk -v value="$longitude" 'BEGIN { exit !(value >= -180 && value <= 180) }' \
    || fail '--longitude must be between -180 and 180'
}
