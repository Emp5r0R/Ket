#!/usr/bin/env bash
# Tests the release preflight's own decisions (version agreement and what
# counts as a hidden endpoint). The checks themselves run against real systems.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-preflight-test.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
# shellcheck source=/dev/null
RELEASE_PREFLIGHT_LIBRARY=1 . "$root/scripts/release-preflight.sh"
set -euo pipefail

fail_test() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

# fixture VERSION... : a tree whose four version files carry these versions.
fixture() {
  rm -rf "$work/tree"
  mkdir -p "$work/tree/crates/ket-update" "$work/tree/apps/ket-desktop/src-tauri"
  printf '[workspace.package]\nrust-version = "1.88"\nversion = "%s"\n[workspace.dependencies]\nserde = { version = "1.0.1" }\n' "$1" > "$work/tree/Cargo.toml"
  printf '{"releases":[{"version":"0.9.0"},{"version":"%s"}]}\n' "$2" > "$work/tree/crates/ket-update/releases.json"
  printf '{"version":"%s"}\n' "$3" > "$work/tree/apps/ket-desktop/src-tauri/tauri.conf.json"
  printf '{"version":"%s"}\n' "$4" > "$work/tree/apps/ket-desktop/package.json"
}

fixture 1.2.0 1.2.0 1.2.0 1.2.0
release_versions "$work/tree" | versions_agree || fail_test 'matching versions disagree'
[[ $(release_versions "$work/tree" | head -n 1) == 'Cargo.toml 1.2.0' ]] || fail_test 'workspace version not read'
for mismatch in '1.2.0 1.1.0 1.2.0 1.2.0' '1.2.0 1.2.0 1.2.1 1.2.0' '1.2.0 1.2.0 1.2.0 1.0.0'; do
  # shellcheck disable=SC2086 # one version per fixture argument
  fixture $mismatch
  if release_versions "$work/tree" | versions_agree; then fail_test "versions $mismatch were taken as agreeing"; fi
done

masked_status 404 || fail_test '404 is the masked answer'
for code in 200 204 401 403 500 000 ''; do
  if masked_status "$code"; then fail_test "status [$code] counted as hidden"; fi
done

printf 'Release preflight tests passed.\n'
