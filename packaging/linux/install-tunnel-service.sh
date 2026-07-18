#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: sudo %s <ket-tunnel-service> <hysteria> [desktop-user]\n' "$0" >&2
  exit 2
}

[[ $# -ge 2 && $# -le 3 ]] || usage
[[ ${EUID} -eq 0 ]] || {
  printf 'This installer must run as root.\n' >&2
  exit 1
}

service_source=$1
engine_source=$2
desktop_user=${3:-${SUDO_USER:-}}
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

[[ -f ${service_source} && -x ${service_source} ]] || {
  printf 'Tunnel service is not an executable file: %s\n' "${service_source}" >&2
  exit 1
}
[[ -f ${engine_source} && -x ${engine_source} ]] || {
  printf 'Hysteria is not an executable file: %s\n' "${engine_source}" >&2
  exit 1
}
[[ -f ${script_dir}/ket-tunnel.service ]] || {
  printf 'The systemd unit is missing beside this installer.\n' >&2
  exit 1
}
[[ -n ${desktop_user} ]] || {
  printf 'Pass the desktop user as the third argument.\n' >&2
  exit 1
}
id "${desktop_user}" >/dev/null 2>&1 || {
  printf 'Desktop user does not exist: %s\n' "${desktop_user}" >&2
  exit 1
}

if ! getent group ket >/dev/null; then
  groupadd --system ket
fi

install -d -o root -g root -m 0755 /usr/libexec/ket
install -d -o root -g ket -m 0750 /etc/ket
install -o root -g root -m 0755 "${service_source}" /usr/libexec/ket/ket-tunnel-service
install -o root -g root -m 0755 "${engine_source}" /usr/libexec/ket/hysteria
install -o root -g root -m 0644 "${script_dir}/ket-tunnel.service" /etc/systemd/system/ket-tunnel.service

token_file=/etc/ket/tunnel.token
[[ ! -L ${token_file} ]] || {
  printf 'Refusing to use a symbolic-link broker token.\n' >&2
  exit 1
}
if [[ ! -e ${token_file} ]]; then
  /usr/libexec/ket/ket-tunnel-service --init-token
fi
[[ -f ${token_file} && $(stat -c '%s' "${token_file}") -eq 32 ]] || {
  printf 'The existing broker token is invalid; it was not overwritten.\n' >&2
  exit 1
}
chown root:ket "${token_file}"
chmod 0640 "${token_file}"
usermod --append --groups ket "${desktop_user}"

systemctl daemon-reload
systemctl enable --now ket-tunnel.service
systemctl --no-pager --full status ket-tunnel.service

printf 'Installed Ket tunnel service. %s must sign in again before opening Ket.\n' "${desktop_user}"
