# shellcheck shell=bash
# The canary server, reached over SSH: running commands, and short-lived
# invites for checks that must enroll for real. Sourced by the release
# preflight and the DNS leak lab; never executed.
#
# Callers set canary_target (user@host) and the canary_ssh_options array.
# shellcheck disable=SC2154

canary() {
  ssh -o BatchMode=yes -o ConnectTimeout=20 -o LogLevel=ERROR "${canary_ssh_options[@]}" "$canary_target" "$@"
}

# canary_invite LABEL DEVICES: creates a 30-minute invite and prints the
# `ket invite` output, which carries the ket:invite: line.
canary_invite() {
  canary sudo -n ket invite --label "$(printf '%q' "$1")" --devices "$2" --minutes 30 --no-qr 2>&1
}

# canary_revoke LABEL: revokes the invite with this label; prints its id.
canary_revoke() {
  local id
  id=$(canary sudo -n ket invites 2>/dev/null | awk -v label="$1" 'index($0, label) { print $1; exit }')
  [[ -n $id ]] && canary sudo -n ket revoke "$id" --yes >/dev/null 2>&1 && printf '%s\n' "$id"
}

# canary_public_url: the canary's KET_PUBLIC_URL.
canary_public_url() {
  canary "sudo -n sed -n 's/^KET_PUBLIC_URL=//p' /opt/ket/.env | tail -n 1" | tr -d '"\r'
}
