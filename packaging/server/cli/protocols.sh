# shellcheck shell=bash
# Turns the protocol names an admin types into the server's protocol limit
# for an invite, and back into names to print. Sourced by the ket command
# and by create-invite.sh; never executed. Needs jq.
#
# OFFERED is the server's GET /v1/admin/protocols reply:
#   [{"protocol":"stealth","name":"stealth","display_name":"HTTPS Stealth"}, ...]

# protocol_access_json LIST OFFERED: prints the limit for LIST ("all", or
# names separated by commas or spaces) as the API's JSON ("all" or
# {"only":[...]}). Fails, naming the protocols on offer, if a name is unknown.
protocol_access_json() {
  local list=$1 offered=$2 wanted
  wanted=$(tr ',' ' ' <<<"$list" | xargs)
  if [[ -z $wanted || ${wanted,,} == all ]]; then
    printf '"all"'
    return 0
  fi
  jq -cn --arg wanted "$wanted" --argjson offered "$offered" '
    ($wanted | ascii_downcase | split(" ") | map(select(length > 0))) as $names
    | [ $names[] as $name
        | ($offered | map(select(
            (.name | ascii_downcase) == $name
            or (.protocol | ascii_downcase) == $name
            or (.display_name | ascii_downcase | gsub("[^a-z0-9]"; "")) == ($name | gsub("[^a-z0-9]"; ""))
          ))) as $match
        | if ($match | length) == 0 then error("unknown protocol \($name)") else $match[0].protocol end ]
    | {only: unique}
  ' 2>/dev/null || {
    printf 'unknown protocol in "%s". This server offers: %s\n' "$list" \
      "$(jq -r 'map(.name) | join(", ")' <<<"$offered")" >&2
    return 1
  }
}

# protocol_names ACCESS OFFERED: prints a limit (as the API returns it) as
# the short names in OFFERED. A protocol the server no longer offers keeps
# its API name and is marked as such.
protocol_names() {
  jq -r --argjson offered "$2" '
    if . == "all" or . == null then "all"
    else .only | map(. as $protocol
      | ($offered | map(select(.protocol == $protocol)) | first | .name)
        // "\($protocol) (not offered)") | join(", ")
    end
  ' <<<"$1"
}
