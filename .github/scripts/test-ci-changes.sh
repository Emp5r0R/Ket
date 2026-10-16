#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
CLASSIFIER="$ROOT/.github/scripts/ci-changes.sh"
jobs=(rust desktop_ui desktop_native linux_package desktop_windows windows_package android container)
passed=0

assert_selection() {
  local label=$1 path=$2
  shift 2
  local -A expected=()
  local job output value
  for job in "$@"; do expected["$job"]=true; done
  output=$(printf '%s\n' "$path" | "$CLASSIFIER")
  for job in "${jobs[@]}"; do
    value=$(awk -F= -v key="$job" '$1 == key { print $2 }' <<< "$output")
    [[ "$value" == "${expected[$job]:-false}" ]] || {
      printf 'FAIL: %s (%s) expected %s=%s, got %s\n%s\n' \
        "$label" "$path" "$job" "${expected[$job]:-false}" "$value" "$output" >&2
      exit 1
    }
  done
  ((passed += 1))
}

assert_selection 'server source' crates/ket-server/src/api.rs rust container
assert_selection 'workspace manifest and version' Cargo.toml \
  rust desktop_native linux_package desktop_windows windows_package container android
assert_selection 'shared contract' crates/ket-core/src/contract.rs \
  rust desktop_native linux_package desktop_windows windows_package container
assert_selection 'client core' crates/ket-client-core/src/client.rs \
  rust desktop_native linux_package desktop_windows windows_package
assert_selection 'invite format and vectors' crates/ket-invite/tests/vectors/invite-v1.json \
  rust desktop_native linux_package desktop_windows windows_package container android
assert_selection 'shared invite format' crates/invite-capsule/src/capsule.rs \
  rust desktop_native linux_package desktop_windows windows_package container android
assert_selection 'Android invite library' crates/ket-invite-android/src/lib.rs \
  rust desktop_native linux_package desktop_windows windows_package container android
assert_selection 'desktop UI' apps/ket-desktop/src/App.tsx desktop_ui linux_package windows_package
assert_selection 'desktop native' apps/ket-desktop/src-tauri/src/lib.rs \
  desktop_native linux_package desktop_windows windows_package
assert_selection 'Android client' apps/ket-android/app/src/main/AndroidManifest.xml android
assert_selection 'Android engine fetch' packaging/fetch-hysteria.sh linux_package windows_package android
assert_selection 'Android APK validator' packaging/test-validate-android-apk.sh android container
assert_selection 'Linux package lifecycle' packaging/verify-linux-deb.sh linux_package
assert_selection 'Windows package lifecycle' packaging/verify-windows-nsis.ps1 windows_package
assert_selection 'server installer' packaging/install-server.sh container
assert_selection 'documentation only' docs/TESTING.md
assert_selection 'unknown future component is conservative' future/new-client/source.any \
  rust desktop_ui desktop_native linux_package desktop_windows windows_package android container

all_output=$("$CLASSIFIER" --all)
[[ $(grep -c '=true$' <<< "$all_output") -eq ${#jobs[@]} ]] || {
  printf 'FAIL: --all must enable every job\n%s\n' "$all_output" >&2
  exit 1
}
((passed += 1))

only_output=$("$CLASSIFIER" --only rust android)
[[ "$only_output" == *$'rust=true'* && "$only_output" == *$'android=true'* ]] || {
  printf 'FAIL: --only did not enable requested jobs\n%s\n' "$only_output" >&2
  exit 1
}
[[ $(grep -c '=true$' <<< "$only_output") -eq 2 ]] || {
  printf 'FAIL: --only enabled extra jobs\n%s\n' "$only_output" >&2
  exit 1
}
((passed += 1))

null_output=$(printf '%s\0' 'docs/file with spaces.md' 'crates/ket-server/src/api.rs' | "$CLASSIFIER" --null)
[[ "$null_output" == *$'rust=true'* && "$null_output" == *$'container=true'* ]] || {
  printf 'FAIL: --null did not classify NUL-delimited paths\n%s\n' "$null_output" >&2
  exit 1
}
((passed += 1))

for command in "$CLASSIFIER --only" "$CLASSIFIER --only unknown" "$CLASSIFIER --invalid"; do
  if bash -c "$command" >/dev/null 2>&1; then
    printf 'FAIL: invalid invocation succeeded: %s\n' "$command" >&2
    exit 1
  fi
  ((passed += 1))
done

printf 'CI change-classifier tests passed (%d cases).\n' "$passed"
