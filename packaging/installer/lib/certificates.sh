# shellcheck shell=bash
# Certificate issuance for the Ket server installer (the renewal timer lives
# in packaging/server/cli/units.sh).
# Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

issue_certificate() {
  printf 'Requesting the TLS certificate...\n'
  "$source_dir/packaging/server/certificates.sh" issue "$domain" "$direct_host" "$email" \
    || fail "Let's Encrypt could not verify $domain. Check that TCP port 80 reaches this server, then rerun."
}
