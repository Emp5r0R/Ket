# shellcheck shell=bash
# Host inspection and preparation for the Ket server installer.
# Sourced, never executed. Everything Ket runs lives in Docker, so the host
# needs only systemd, Docker with Compose v2, and a few base tools.
# Globals are shared with install-server.sh and the other installer libraries.
# shellcheck disable=SC2034,SC2154

readonly MINIMUM_COMPOSE_VERSION=2.17
apt_updated=false

# Loads ID, ID_LIKE, VERSION_ID, and codenames from os-release (override with
# KET_OS_RELEASE for tests).
load_os_release() {
  local file=${KET_OS_RELEASE:-/etc/os-release}
  [[ -r $file ]] || fail 'cannot detect the Linux distribution (no /etc/os-release)'
  ID='' ID_LIKE='' VERSION_ID='' VERSION_CODENAME='' UBUNTU_CODENAME='' PRETTY_NAME=''
  # shellcheck disable=SC1090 # os-release is a fixed-format system file
  . "$file"
}

os_like() {
  local candidate
  for candidate in "$@"; do
    [[ ${ID:-} == "$candidate" || " ${ID_LIKE:-} " == *" $candidate "* ]] && return 0
  done
  return 1
}

# Prints the package-manager family: apt, dnf, zypper, pacman, or unknown.
os_family() {
  if os_like debian ubuntu raspbian; then
    printf apt
  elif os_like fedora rhel centos rocky almalinux ol amzn; then
    printf dnf
  elif os_like opensuse opensuse-leap opensuse-tumbleweed sles suse; then
    printf zypper
  elif os_like arch; then
    printf pacman
  else
    printf unknown
  fi
}

cpu_arch() {
  case $(uname -m) in
    x86_64|amd64) printf amd64 ;;
    aarch64|arm64) printf arm64 ;;
    *) fail "only amd64 and arm64 servers are supported (found $(uname -m))" ;;
  esac
}

require_systemd() {
  if ! command -v systemctl >/dev/null 2>&1 || [[ ! -d /run/systemd/system ]]; then
    fail 'systemd is required to run Ket services'
  fi
}

pkg_install() {
  local family=$1
  shift
  case $family in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      if ! $apt_updated; then
        apt-get update
        apt_updated=true
      fi
      apt-get install -y "$@"
      ;;
    dnf) dnf install -y "$@" ;;
    zypper) zypper --non-interactive install "$@" ;;
    pacman) pacman -Sy --noconfirm --needed "$@" ;;
    *) return 1 ;;
  esac
}

# Installs any missing required tools, then optional ones on a best-effort basis.
ensure_base_tools() {
  local family=$1 tool missing=()
  for tool in curl tar openssl jq; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
  done
  if ((${#missing[@]})); then
    printf 'Installing required tools: %s\n' "${missing[*]}"
    pkg_install "$family" ca-certificates "${missing[@]}" \
      || fail "install these tools and rerun: ${missing[*]}"
  fi
  command -v qrencode >/dev/null 2>&1 || pkg_install "$family" qrencode >/dev/null 2>&1 \
    || printf 'qrencode is unavailable; invites will be printed as text only.\n'
  if ! command -v ss >/dev/null 2>&1; then
    case $family in
      dnf) pkg_install "$family" iproute >/dev/null 2>&1 ;;
      *) pkg_install "$family" iproute2 >/dev/null 2>&1 ;;
    esac || printf 'ss is unavailable; port conflicts will surface when Ket starts.\n'
  fi
}

docker_compose_ready() {
  local compose_version
  docker info >/dev/null 2>&1 || return 1
  compose_version=$(docker compose version --short 2>/dev/null | sed 's/^v//') || return 1
  [[ $compose_version =~ ^[0-9]+([.][0-9]+){1,2}$ ]] || return 1
  version_ge "$compose_version" "$MINIMUM_COMPOSE_VERSION"
}

# Docker's apt repository for this distribution or its upstream.
apt_docker_repository() {
  if os_like ubuntu; then
    printf 'ubuntu %s' "${UBUNTU_CODENAME:-${VERSION_CODENAME:-}}"
  else
    printf 'debian %s' "${VERSION_CODENAME:-}"
  fi
}

# Docker's dnf repository file URL for this distribution.
dnf_docker_repository() {
  if [[ ${ID:-} == fedora ]]; then
    printf 'https://download.docker.com/linux/fedora/docker-ce.repo'
  elif [[ ${ID:-} == rhel ]]; then
    printf 'https://download.docker.com/linux/rhel/docker-ce.repo'
  elif os_like rhel centos rocky almalinux ol; then
    printf 'https://download.docker.com/linux/centos/docker-ce.repo'
  else
    return 1
  fi
}

install_docker_apt() {
  local arch=$1 repository distro codename package conflicts=()
  for package in docker.io docker-compose docker-compose-v2 docker-doc podman-docker containerd runc; do
    if dpkg-query -W -f='${db:Status-Abbrev}' "$package" 2>/dev/null | grep -q '^ii'; then
      conflicts+=("$package")
    fi
  done
  ((${#conflicts[@]} == 0)) || fail "remove these packages first, they conflict with Docker's packages: ${conflicts[*]}"
  repository=$(apt_docker_repository)
  distro=${repository% *}
  codename=${repository#* }
  [[ -n $codename ]] || fail 'could not determine the distribution codename for Docker'
  pkg_install apt ca-certificates curl gnupg
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL "https://download.docker.com/linux/$distro/gpg" -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
  cat > /etc/apt/sources.list.d/docker.sources <<EOF
Types: deb
URIs: https://download.docker.com/linux/$distro
Suites: $codename
Components: stable
Architectures: $arch
Signed-By: /etc/apt/keyrings/docker.asc
EOF
  apt_updated=false
  pkg_install apt docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
}

install_docker_dnf() {
  local repository
  repository=$(dnf_docker_repository) \
    || fail "install Docker Engine with the Compose plugin for ${PRETTY_NAME:-this system}, then rerun"
  if rpm -q podman-docker >/dev/null 2>&1; then
    fail 'remove podman-docker first; it conflicts with Docker Engine'
  fi
  curl -fsSL "$repository" -o /etc/yum.repos.d/docker-ce.repo
  pkg_install dnf docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
}

# Uses an existing Docker with Compose v2, or installs Docker's official packages.
ensure_docker() {
  local family=$1 arch=$2
  if ! docker_compose_ready; then
    if command -v docker >/dev/null 2>&1 && ! docker compose version >/dev/null 2>&1; then
      fail 'Docker is installed without Compose v2; install the docker-compose-plugin package, then rerun'
    fi
    if ! command -v docker >/dev/null 2>&1; then
      printf 'Installing Docker Engine from Docker'\''s official repository...\n'
      case $family in
        apt) install_docker_apt "$arch" ;;
        dnf) install_docker_dnf ;;
        *) fail "install Docker Engine with the Compose plugin for ${PRETTY_NAME:-this system}, then rerun" ;;
      esac
    fi
    systemctl enable --now docker
  fi
  docker_compose_ready \
    || fail "Docker with Compose $MINIMUM_COMPOSE_VERSION or newer is required and must be running"
}

# WireGuard needs kernel support (built into Linux 5.6+, backported by many distributions).
kernel_supports_wireguard() {
  [[ -d /sys/module/wireguard ]] || modprobe -q wireguard 2>/dev/null
}

apply_host_firewall_rules() {
  local range="$shadowsocks_start-$shadowsocks_end"
  if command -v ufw >/dev/null 2>&1 && ufw status | grep -q '^Status: active'; then
    printf 'Adding Ket rules to the active UFW firewall...\n'
    ufw allow 80/tcp comment 'Ket certificate renewal'
    ufw allow 443/tcp comment 'Ket HTTPS transports'
    ufw allow 443/udp comment 'Ket Hysteria2'
    ufw allow 9443/tcp comment 'Ket OpenVPN TLS'
    ufw allow "${range/-/:}/tcp" comment 'Ket Shadowsocks TCP'
    ufw allow "${range/-/:}/udp" comment 'Ket Shadowsocks UDP'
  elif command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    printf 'Adding Ket rules to the active firewalld zone...\n'
    local port
    for port in 80/tcp 443/tcp 443/udp 9443/tcp "$range/tcp" "$range/udp"; do
      firewall-cmd --quiet --permanent --add-port="$port"
    done
    firewall-cmd --quiet --reload
  else
    printf 'No active UFW or firewalld; the host firewall was not changed.\n'
  fi
}
