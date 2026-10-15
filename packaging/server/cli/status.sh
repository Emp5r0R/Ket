# shellcheck shell=bash
# A one-screen summary of this server. Sourced by the ket command; never executed.
# shellcheck disable=SC2034,SC2154

# Prints "SERVICE STATE HEALTH" lines from `docker compose ps`, which emits a
# JSON array on older Compose releases and one object per line on newer ones.
service_states() {
  compose ps --all --format json 2>/dev/null | jq -rs '
    (if length == 1 and (.[0] | type) == "array" then .[0] else . end)[]
    | "\(.Service) \(.State) \(if (.Health // "") == "" then "-" else .Health end)"'
}

# certificate_days_left FILE: whole days until the certificate expires.
certificate_days_left() {
  local end
  end=$(openssl x509 -enddate -noout -in "$1" 2>/dev/null | sed 's/^notAfter=//') || return 1
  [[ -n $end ]] || return 1
  printf '%s' $(( ($(date -d "$end" +%s) - $(date +%s)) / 86400 ))
}

metric_value() {
  sed -n "s/^$1 //p" <<<"$2" | head -n 1
}

cmd_status() {
  require_root
  require_installation
  require_commands curl jq openssl
  local metrics days check
  info "Ket $(installed_ref) at $(env_value KET_PUBLIC_URL)"
  info "Installed in $install_dir; automatic updates: $(auto_update_policy)"
  check="$(ket_state_dir)/update-check.json"
  if [[ -r $check ]]; then
    jq -r '"Last update check \(.checked): latest \(.latest)\(if .category != "" then " (" + .category + ")" else "" end), \(.outcome)"' "$check"
  fi
  info ''
  info 'Services:'
  service_states | sed 's/^/  /' || warn 'could not list the services; is Docker running?'
  info ''
  if curl --fail --silent --max-time 5 "$(control_url /readyz)" >/dev/null 2>&1; then
    info 'Control plane: ready'
    metrics=$(curl --fail --silent --max-time 5 "$(control_url /metrics)" || true)
    info "Connected devices: $(metric_value ket_active_sessions "$metrics")/$(metric_value ket_session_capacity "$metrics")"
  else
    info 'Control plane: NOT READY (run: sudo ket doctor)'
  fi
  if days=$(certificate_days_left "$install_dir/secrets/tls/fullchain.pem"); then
    info "TLS certificate: expires in $days days"
  else
    info 'TLS certificate: not found'
  fi
}
