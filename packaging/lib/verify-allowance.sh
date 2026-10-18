# shellcheck shell=bash
# Data allowance checks against a real control plane and engine, for the
# engine traffic tests. Sourced by packaging/verify-*-traffic.sh; never
# executed. Callers set control_url (http://127.0.0.1:PORT) and admin_token.
# shellcheck disable=SC2154

# The smallest allowance the server accepts: 1 MB a day.
readonly ALLOWANCE_BYTES=1048576

# allowance_grant LABEL: creates a one-device grant with ALLOWANCE_BYTES a
# day and prints the server's reply (id, access_code, ...).
allowance_grant() {
  curl --fail --silent --show-error \
    -H "Authorization: Bearer $admin_token" -H 'Content-Type: application/json' \
    --data "$(jq -cn --arg label "$1" --argjson bytes "$ALLOWANCE_BYTES" \
      '{label:$label,max_connections:1,valid_for_minutes:60,allowance:{bytes:$bytes,period:"day"}}')" \
    "$control_url/v1/admin/access-grants"
}

# allowance_used GRANT_ID: what the grant has used this period, in bytes.
allowance_used() {
  curl --fail --silent --show-error -H "Authorization: Bearer $admin_token" \
    "$control_url/v1/admin/access-grants/$1" | jq -er '.allowance.used_bytes'
}

# wait_allowance_counted GRANT_ID MINIMUM: waits (up to 40 s, past two meter
# readings) until the grant has counted at least MINIMUM bytes.
wait_allowance_counted() {
  local used=0
  for _ in {1..80}; do
    used=$(allowance_used "$1")
    ((used >= $2)) && return 0
    sleep 0.5
  done
  printf 'The grant counted %s bytes, not the %s that went through the engine.\n' "$used" "$2" >&2
  return 1
}

# use_up_allowance GRANT_ID PUSH: runs PUSH (which moves some data through
# the engine and may fail once Ket disconnects the device) until the grant
# has used its allowance, for up to 90 s.
use_up_allowance() {
  local used=0
  for _ in {1..180}; do
    "$2" >/dev/null 2>&1 || true
    used=$(allowance_used "$1")
    ((used >= ALLOWANCE_BYTES)) && return 0
    sleep 0.5
  done
  printf 'The grant never used its allowance (%s of %s bytes counted).\n' "$used" "$ALLOWANCE_BYTES" >&2
  return 1
}

# enroll_with ACCESS_CODE LABEL: the HTTP status and body of an enrollment.
enroll_with() {
  curl --silent --show-error -H 'Content-Type: application/json' \
    --data "$(jq -cn --arg code "$1" --arg name "$2" '{access_code:$code,client_name:$name}')" \
    --write-out '\n%{http_code}' "$control_url/v1/sessions"
}

# assert_allowance_refused ACCESS_CODE: a used-up grant refuses new devices
# with 423 grant_allowance_used, and lets them in once its count is reset.
assert_allowance_refused() {
  local grant_id=${1:0:10} reply status body
  reply=$(enroll_with "$1" 'Allowance refusal test')
  status=${reply##*$'\n'}
  body=${reply%$'\n'*}
  [[ $status == 423 && $(jq -r '.code' <<< "$body") == grant_allowance_used ]] || {
    printf 'A used-up allowance did not refuse enrollment (HTTP %s).\n' "$status" >&2
    return 1
  }
  curl --fail --silent --show-error -X PATCH \
    -H "Authorization: Bearer $admin_token" -H 'Content-Type: application/json' \
    --data '{"reset_usage":true}' "$control_url/v1/admin/access-grants/$grant_id" >/dev/null
  reply=$(enroll_with "$1" 'Allowance reset test')
  status=${reply##*$'\n'}
  [[ $status == 201 ]] || {
    printf 'A reset allowance still refused enrollment (HTTP %s).\n' "$status" >&2
    return 1
  }
  curl --fail --silent --show-error -X DELETE \
    -H "Authorization: Bearer $(jq -er '.session_token' <<< "${reply%$'\n'*}")" \
    "$control_url/v1/sessions/current" >/dev/null
}
