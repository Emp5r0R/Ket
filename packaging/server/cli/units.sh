# shellcheck shell=bash
# Host integration for an installation: systemd timers and the `ket` command.
# Sourced by the ket command and by the installer; never executed.
# Uses the global install_dir. KET_SYSTEMD_DIR and KET_BIN_DIR redirect the
# files for tests, which then also skips systemctl.
# shellcheck disable=SC2034,SC2154

systemd_enable_timer() {
  systemd_is_simulated && return 0
  systemctl daemon-reload
  systemctl enable --now "$1"
}

# A twice-daily timer runs the containerized certificate renewal.
install_renewal_timer() {
  local unit_dir
  unit_dir=$(ket_systemd_dir)
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
  systemd_enable_timer ket-certificates.timer
}

# A daily timer checks for a new release and applies it when KET_AUTO_UPDATE
# allows (see docs/UPDATES.md).
install_update_timer() {
  local unit_dir
  unit_dir=$(ket_systemd_dir)
  cat > "$unit_dir/ket-update.service" <<UNIT
[Unit]
Description=Check for and apply Ket server updates
After=network-online.target docker.service
Wants=network-online.target
Requires=docker.service

[Service]
Type=oneshot
ExecStart=$install_dir/packaging/server/ket.sh update --auto
UNIT
  cat > "$unit_dir/ket-update.timer" <<'UNIT'
[Unit]
Description=Check for Ket server updates once a day

[Timer]
OnCalendar=*-*-* 04:41:00
RandomizedDelaySec=3h
Persistent=true

[Install]
WantedBy=timers.target
UNIT
  systemd_enable_timer ket-update.timer
}

install_command_link() {
  local bin_dir
  bin_dir=$(ket_bin_dir)
  install -d -m 0755 "$bin_dir"
  ln -sfn "$install_dir/packaging/server/ket.sh" "$bin_dir/ket"
}

# Servers installed by v1.0.1 renew through the host's certbot and a deploy
# hook; they keep that instead of the container timer.
uses_host_certbot() {
  [[ -e ${KET_LETSENCRYPT_DIR:-/etc/letsencrypt}/renewal-hooks/deploy/ket-server ]]
}

# Installs everything an installation needs outside its directory.
install_host_integration() {
  uses_host_certbot || install_renewal_timer
  install_update_timer
  install_command_link
}

remove_host_integration() {
  local unit_dir unit
  unit_dir=$(ket_systemd_dir)
  if ! systemd_is_simulated; then
    systemctl disable --now ket-certificates.timer ket-update.timer >/dev/null 2>&1 || true
  fi
  for unit in ket-certificates.service ket-certificates.timer ket-update.service ket-update.timer; do
    rm -f "$unit_dir/$unit"
  done
  systemd_is_simulated || systemctl daemon-reload
  rm -f "$(ket_bin_dir)/ket"
  rm -f "${KET_LETSENCRYPT_DIR:-/etc/letsencrypt}/renewal-hooks/deploy/ket-server"
}
