# shellcheck shell=bash
# Certificate issuance and the renewal timer for the Ket server installer.
# Sourced, never executed.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

issue_certificate() {
  printf 'Requesting the TLS certificate...\n'
  "$source_dir/packaging/server/certificates.sh" issue "$domain" "$direct_host" "$email" \
    || fail "Let's Encrypt could not verify $domain. Check that TCP port 80 reaches this server, then rerun."
}

# A twice-daily systemd timer runs the containerized renewal from the install
# directory. KET_SYSTEMD_DIR redirects the units for tests.
install_renewal_timer() {
  local unit_dir=${KET_SYSTEMD_DIR:-/etc/systemd/system}
  cat > "$unit_dir/ket-certificates.service" <<UNIT
[Unit]
Description=Renew the Ket TLS certificate
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
WorkingDirectory=$install_dir
ExecStart=$install_dir/packaging/server/certificates.sh renew
UNIT
  cat > "$unit_dir/ket-certificates.timer" <<'UNIT'
[Unit]
Description=Check the Ket TLS certificate twice a day

[Timer]
OnCalendar=*-*-* 03,15:17:00
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
UNIT
  if [[ -z ${KET_SYSTEMD_DIR:-} ]]; then
    systemctl daemon-reload
    systemctl enable --now ket-certificates.timer
  fi
}
