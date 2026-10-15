# shellcheck shell=bash
# Host locations used by the ket command and the installer. Sourced, never
# executed. Each is overridable so tests never touch the host.

ket_bin_dir() { printf '%s' "${KET_BIN_DIR:-/usr/local/bin}"; }
ket_systemd_dir() { printf '%s' "${KET_SYSTEMD_DIR:-/etc/systemd/system}"; }
ket_backup_dir() { printf '%s' "${KET_BACKUP_DIR:-/var/backups/ket}"; }
ket_state_dir() { printf '%s' "${KET_STATE_DIR:-/var/lib/ket}"; }
ket_tty() { printf '%s' "${KET_TTY:-/dev/tty}"; }

# True when systemd units are written for tests rather than the host.
systemd_is_simulated() { [[ -n ${KET_SYSTEMD_DIR:-} ]]; }
