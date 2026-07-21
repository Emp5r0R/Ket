#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'Ket DEB verification failed: %s\n' "$1" >&2
  exit 1
}

[[ $# -eq 1 ]] || fail "usage: sudo KET_PACKAGE_TEST_ALLOW_HOST_MUTATION=1 $0 <package.deb>"
[[ ${EUID} -eq 0 ]] || fail "the lifecycle verifier must run as root"
[[ ${KET_PACKAGE_TEST_ALLOW_HOST_MUTATION:-} == 1 ]] || {
  fail "set KET_PACKAGE_TEST_ALLOW_HOST_MUTATION=1 on an ephemeral test host"
}

for command in cut dpkg dpkg-deb dpkg-query find getent grep id ldd realpath sed sha256sum stat tr; do
  command -v "${command}" >/dev/null || fail "required command is missing: ${command}"
done

package_path=$(realpath -- "$1")
[[ -f ${package_path} ]] || fail "package does not exist: ${package_path}"

package_name=$(dpkg-deb --field "${package_path}" Package)
package_version=$(dpkg-deb --field "${package_path}" Version)
package_architecture=$(dpkg-deb --field "${package_path}" Architecture)
host_architecture=$(dpkg --print-architecture)

[[ ${package_name} == ket ]] || fail "unexpected package name: ${package_name}"
[[ -n ${package_version} ]] || fail "package version is empty"
[[ ${package_architecture} == "${host_architecture}" ]] || {
  fail "package architecture ${package_architecture} does not match host ${host_architecture}"
}

if dpkg-query --show --showformat='${db:Status-Status}' "${package_name}" 2>/dev/null \
  | grep -qx installed; then
  fail "${package_name} is already installed; use a disposable host"
fi
[[ ! -e /etc/ket/tunnel.token ]] || fail "/etc/ket/tunnel.token already exists"

scratch=$(mktemp -d)
package_present=false
cleanup() {
  status=$?
  trap - EXIT
  if [[ ${package_present} == true ]]; then
    dpkg --purge "${package_name}" >/dev/null 2>&1 || true
  fi
  rm -rf -- "${scratch}"
  exit "${status}"
}
trap cleanup EXIT

dpkg-deb --control "${package_path}" "${scratch}/control"
dpkg-deb --extract "${package_path}" "${scratch}/root"

for script in postinst prerm postrm; do
  [[ -f ${scratch}/control/${script} ]] || fail "maintainer script is missing: ${script}"
  /bin/sh -n "${scratch}/control/${script}"
done

required_payloads=(
  usr/bin/ket-desktop
  usr/libexec/ket/ket-tunnel-service
  usr/libexec/ket/hysteria
  usr/libexec/ket/sslocal
  usr/libexec/ket/xray
  usr/libexec/ket/tun2proxy
)
for payload in "${required_payloads[@]}"; do
  [[ -f ${scratch}/root/${payload} && -x ${scratch}/root/${payload} ]] || {
    fail "required executable payload is missing: /${payload}"
  }
done

desktop_entry=${scratch}/root/usr/share/applications/Ket.desktop
[[ -f ${desktop_entry} ]] || fail "desktop entry is missing"
grep -qx 'Exec=ket-desktop' "${desktop_entry}" || fail "desktop entry has the wrong executable"
icon_name=$(sed -n 's/^Icon=//p' "${desktop_entry}")
[[ -n ${icon_name} && ${icon_name} != /* ]] || fail "desktop entry must use a themed icon name"
icon_path=$(find "${scratch}/root/usr/share/icons" -type f \
  \( -name "${icon_name}.png" -o -name "${icon_name}.svg" -o -name "${icon_name}.xpm" \) \
  -print -quit 2>/dev/null || true)
[[ -n ${icon_path} ]] || fail "desktop icon ${icon_name} is not packaged"

test_user=${KET_PACKAGE_TEST_USER:-${SUDO_USER:-}}
[[ -n ${test_user} && ${test_user} != root ]] || fail "set KET_PACKAGE_TEST_USER to a non-root test user"
id "${test_user}" >/dev/null 2>&1 || fail "test user does not exist: ${test_user}"

install_package() {
  package_present=true
  env SUDO_USER="${test_user}" dpkg --install "${package_path}"
  dpkg-query --show --showformat='${Status}' "${package_name}" | grep -qx 'install ok installed'
}

verify_installation() {
  [[ $(stat -c '%U:%G:%a' /usr/libexec/ket) == root:root:755 ]] || fail "invalid engine directory ownership or mode"
  [[ $(stat -c '%U:%G:%a' /etc/ket) == root:ket:750 ]] || fail "invalid broker directory ownership or mode"
  [[ $(stat -c '%U:%G:%a' /etc/ket/tunnel.token) == root:ket:640 ]] || fail "invalid broker token ownership or mode"
  [[ $(stat -c '%s' /etc/ket/tunnel.token) -eq 32 ]] || fail "broker token must contain 32 bytes"
  for payload in "${required_payloads[@]}"; do
    [[ $(stat -c '%U:%G:%a' "/${payload}") == root:root:755 ]] || fail "invalid payload ownership or mode: /${payload}"
  done
  id -nG "${test_user}" | tr ' ' '\n' | grep -qx ket || fail "test user was not added to the ket group"
  if ldd /usr/bin/ket-desktop | grep -q 'not found'; then
    fail "desktop executable has unresolved shared libraries"
  fi
  /usr/libexec/ket/hysteria version >/dev/null
  /usr/libexec/ket/sslocal --version >/dev/null
  /usr/libexec/ket/xray version >/dev/null
  /usr/libexec/ket/tun2proxy --version >/dev/null
  if [[ -d /run/systemd/system ]]; then
    systemctl is-enabled --quiet ket-tunnel.service || fail "tunnel service is not enabled"
    systemctl is-active --quiet ket-tunnel.service || fail "tunnel service is not active"
  fi
}

install_package
verify_installation
original_token_sha256=$(sha256sum /etc/ket/tunnel.token | cut -d ' ' -f 1)

install_package
verify_installation
reinstalled_token_sha256=$(sha256sum /etc/ket/tunnel.token | cut -d ' ' -f 1)
[[ ${reinstalled_token_sha256} == "${original_token_sha256}" ]] || {
  fail "reinstall replaced the broker token"
}

dpkg --remove "${package_name}"
[[ ! -e /usr/bin/ket-desktop ]] || fail "desktop executable remains after removal"
[[ ! -e /usr/libexec/ket/ket-tunnel-service ]] || fail "tunnel service remains after removal"
[[ -f /etc/ket/tunnel.token ]] || fail "removal deleted persistent broker state"

dpkg --purge "${package_name}"
package_present=false
[[ ! -e /etc/ket/tunnel.token ]] || fail "purge retained the broker token"
[[ ! -d /etc/ket ]] || fail "purge retained the empty broker directory"

printf 'Ket DEB %s lifecycle verification passed for %s.\n' "${package_version}" "${package_architecture}"
