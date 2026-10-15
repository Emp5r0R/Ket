#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MODE=${1:-unit}
started_at=$SECONDS
steps=0

usage() {
  cat <<'EOF'
Usage: ./scripts/test-all.sh [unit|quality|integration|all|rust|desktop-ui|desktop-native|android|server]

  unit           Run every portable unit/contract suite (default).
  quality        Run formatting, lint, and compile/build gates.
  integration    Run the Docker edge probe harness and real local OpenVPN, Shadowsocks, and XHTTP engine harnesses.
  all            Run unit, quality, and integration gates.
  rust           Run Rust formatting, all-target tests, and strict Clippy.
  desktop-ui     Run desktop UI tests and its production build.
  desktop-native Run the optimized Tauri/Rust native tests.
  android        Run Android JVM unit tests and lint.
  server         Run shell syntax, installer, certificate, environment, and CI-routing tests.
EOF
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$1" >&2
    exit 127
  }
}

run_step() {
  local label=$1
  shift
  local step_started=$SECONDS
  ((steps += 1))
  printf '\n[%d] %s\n' "$steps" "$label"
  "$@"
  printf 'PASS: %s (%ss)\n' "$label" "$((SECONDS - step_started))"
}

run_in() {
  local directory=$1 label=$2
  shift 2
  local step_started=$SECONDS
  ((steps += 1))
  printf '\n[%d] %s\n' "$steps" "$label"
  (cd "$directory" && "$@")
  printf 'PASS: %s (%ss)\n' "$label" "$((SECONDS - step_started))"
}

rust_unit() {
  require_command cargo
  run_step 'Rust workspace unit and contract tests' \
    cargo test --locked --workspace --exclude ket-desktop --all-targets --all-features
}

rust_quality() {
  require_command cargo
  run_step 'Rust formatting' cargo fmt --all -- --check
  run_step 'Rust strict Clippy' \
    cargo clippy --locked --workspace --exclude ket-desktop --all-targets --all-features -- -D warnings
}

desktop_ui_unit() {
  require_command npm
  [[ -d "$ROOT/apps/ket-desktop/node_modules" ]] || {
    printf 'Desktop dependencies are missing; run: npm ci --prefix apps/ket-desktop\n' >&2
    exit 1
  }
  run_step 'Desktop UI unit and interaction tests' \
    npm --prefix "$ROOT/apps/ket-desktop" test -- --run
}

desktop_ui_quality() {
  require_command npm
  [[ -d "$ROOT/apps/ket-desktop/node_modules" ]] || {
    printf 'Desktop dependencies are missing; run: npm ci --prefix apps/ket-desktop\n' >&2
    exit 1
  }
  run_step 'Desktop TypeScript and production build' \
    npm --prefix "$ROOT/apps/ket-desktop" run build
}

desktop_native_unit() {
  require_command cargo
  if [[ $(uname -s) == Linux ]]; then
    require_command pkg-config
    local packages=(libsoup-3.0 webkit2gtk-4.1 javascriptcoregtk-4.1)
    local missing=() package
    for package in "${packages[@]}"; do
      pkg-config --exists "$package" || missing+=("$package")
    done
    if ((${#missing[@]})); then
      printf 'Missing Tauri Linux development packages: %s\n' "${missing[*]}" >&2
      printf 'Ubuntu/Debian: sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libxdo-dev patchelf\n' >&2
      exit 1
    fi
  fi
  run_step 'Optimized desktop native tests' \
    cargo test --locked --release -p ket-desktop --lib
}

android_unit() {
  require_command java
  [[ -x "$ROOT/apps/ket-android/gradlew" ]] || {
    printf 'Android Gradle wrapper is missing or is not executable.\n' >&2
    exit 1
  }
  run_in "$ROOT/apps/ket-android" 'Android JVM unit tests' \
    ./gradlew --no-daemon --console=plain testDebugUnitTest
}

android_quality() {
  require_command java
  run_in "$ROOT/apps/ket-android" 'Android lint' \
    ./gradlew --no-daemon --console=plain lintDebug
}

git_tracked_scripts_are_executable() {
  # Some checkouts (core.fileMode=false) hide a missing executable bit locally.
  local non_executable
  non_executable=$(git -C "$ROOT" ls-files -s -- '*.sh' | awk '$1 != "100755" {print $4}')
  [[ -z "$non_executable" ]] || {
    printf 'Shell scripts are not executable in Git (git update-index --chmod=+x):\n%s\n' "$non_executable" >&2
    return 1
  }
}

server_contracts() {
  require_command bash
  require_command gpg
  local scripts=()
  while IFS= read -r script; do scripts+=("$script"); done < <(
    { printf '%s\n' "$ROOT/install.sh"
      find "$ROOT/.github/scripts" "$ROOT/packaging" "$ROOT/scripts" -type f -name '*.sh' -print; } \
      | LC_ALL=C sort
  )
  run_step 'Shell syntax for every maintained Bash script' bash -n "${scripts[@]}"
  if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    run_step 'Tracked shell scripts are executable in Git' git_tracked_scripts_are_executable
  fi
  run_step 'Server installer command-line tests' "$ROOT/packaging/test-install-server.sh"
  run_step 'Server installer library tests' "$ROOT/packaging/test-installer-lib.sh"
  run_step 'One-command bootstrap tests' "$ROOT/packaging/test-bootstrap.sh"
  run_step 'Compose overlay selection tests' "$ROOT/packaging/test-compose-overlays.sh"
  run_step 'Public edge renderer and template contract tests' "$ROOT/packaging/test-render-edge.sh"
  run_step 'Invite helper contract tests' "$ROOT/packaging/test-create-invite.sh"
  run_step 'Certificate trust-bundle tests' "$ROOT/packaging/test-certificate-trust-bundle.sh"
  run_step 'All-transport environment preflight tests' "$ROOT/packaging/test-validate-env.sh"
  run_step 'Release checksum and OpenPGP signing tests' "$ROOT/packaging/test-sign-release.sh"
  run_step 'CI change-classifier tests' "$ROOT/.github/scripts/test-ci-changes.sh"
  run_step 'CI Ubuntu dependency installer tests' \
    "$ROOT/.github/scripts/test-install-ubuntu-dependencies.sh"
  if command -v shellcheck >/dev/null 2>&1; then
    run_step 'ShellCheck static analysis' shellcheck "${scripts[@]}"
  else
    printf '\nSKIP: ShellCheck is optional and is not installed.\n'
  fi
}

portable_unit() {
  rust_unit
  desktop_native_unit
  desktop_ui_unit
  android_unit
  server_contracts
}

quality() {
  rust_quality
  desktop_ui_quality
  android_quality
  if command -v git >/dev/null 2>&1 && git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    run_step 'Whitespace and patch integrity' git -C "$ROOT" diff --check
  fi
}

integration() {
  run_step 'Public edge probe resistance (Docker)' "$ROOT/packaging/verify-edge-proxy.sh"
  run_step 'Installer provisioning with real key generation (Docker)' "$ROOT/packaging/verify-installer-provisioning.sh"
  run_step 'OpenVPN over stunnel engine lifecycle' "$ROOT/packaging/verify-openvpn-handshake.sh"
  run_step 'Shadowsocks 2022 engine traffic lifecycle' "$ROOT/packaging/verify-shadowsocks-traffic.sh"
  run_step 'XHTTP TLS Stealth engine traffic lifecycle' "$ROOT/packaging/verify-xhttp-tls-traffic.sh"
}

case "$MODE" in
  unit)
    portable_unit
    ;;
  quality)
    quality
    ;;
  integration)
    integration
    ;;
  all)
    portable_unit
    quality
    integration
    ;;
  rust)
    rust_unit
    rust_quality
    ;;
  desktop-ui)
    desktop_ui_unit
    desktop_ui_quality
    ;;
  desktop-native)
    desktop_native_unit
    ;;
  android)
    android_unit
    android_quality
    ;;
  server)
    server_contracts
    ;;
  -h|--help|help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    printf '\nUnknown test mode: %s\n' "$MODE" >&2
    exit 2
    ;;
esac

printf '\nAll %d test steps passed in %ss.\n' "$steps" "$((SECONDS - started_at))"
