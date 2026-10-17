#!/usr/bin/env bash
# Release preflight: proves a release candidate on real systems before it is
# tagged. A tag goes only on a commit that passed `all` while the canary
# server ran exactly that commit (docs/RELEASE.md, "Release candidate").
#
#   scripts/release-preflight.sh local
#   scripts/release-preflight.sh canary SSH_TARGET [--ssh-key FILE]
#   scripts/release-preflight.sh all SSH_TARGET [--ssh-key FILE]
#
# local   The committed and pushed candidate: versions agree, CI is green for
#         this commit, the full test suite passes, and the real installer and
#         bootstrap (no stubs) behave on this host.
# canary  The canary server runs this commit (or its tag), `ket doctor`
#         passes, operational endpoints are hidden from the public name
#         (through any CDN or tunnel), and a fresh invite enrolls with the
#         shipped client code.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
readonly ROOT
readonly REPOSITORY=Emp5r0R/Ket
# shellcheck source=scripts/lib/canary.sh
. "$ROOT/scripts/lib/canary.sh"
# Paths the public name must never serve; each must answer 404 like any
# unknown path.
readonly PRIVATE_PATHS=(/metrics /readyz /healthz /v1/node/status /v1/admin/access-grants /internal/v1/openvpn/auth)

failures=0

step() { printf '\n== %s\n' "$1"; }
pass() { printf '  ok    %s\n' "$1"; }
problem() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
die() {
  printf 'release-preflight: %s\n' "$1" >&2
  exit 2
}

# --- versions -------------------------------------------------------------------

workspace_version() { sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1; }

# release_versions ROOT: "<label> <version>" for every place that names the
# release version; they must all agree.
release_versions() {
  local root=$1
  printf 'Cargo.toml %s\n' "$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
  printf 'releases.json %s\n' "$(jq -r '.releases[-1].version' "$root/crates/ket-update/releases.json")"
  printf 'tauri.conf.json %s\n' "$(jq -r '.version' "$root/apps/ket-desktop/src-tauri/tauri.conf.json")"
  printf 'package.json %s\n' "$(jq -r '.version' "$root/apps/ket-desktop/package.json")"
}

# versions_agree: true when every line of release_versions has one version.
versions_agree() { [[ $(awk '{ print $2 }' | LC_ALL=C sort -u | wc -l) -eq 1 ]]; }

check_versions() {
  step 'Versions'
  local versions version head tagged
  versions=$(release_versions "$ROOT")
  version=$(workspace_version)
  if versions_agree <<<"$versions"; then pass "every version file says $version"; else
    problem "version files disagree: $(tr '\n' ';' <<<"$versions")"
  fi
  head=$(git -C "$ROOT" rev-parse HEAD)
  if tagged=$(git -C "$ROOT" rev-parse -q --verify "refs/tags/v$version^{commit}" 2>/dev/null); then
    if [[ $tagged == "$head" ]]; then pass "v$version already tags this commit"; else
      problem "v$version already tags another commit ($tagged); bump the version"
    fi
  else
    pass "v$version is not tagged yet"
  fi
}

# --- the committed candidate -----------------------------------------------------

check_commit() {
  step 'Commit'
  local head conclusion
  head=$(git -C "$ROOT" rev-parse HEAD)
  if [[ -z $(git -C "$ROOT" status --porcelain --untracked-files=no) ]]; then pass 'no uncommitted changes to tracked files'; else
    problem 'tracked files have uncommitted changes; the candidate must be a commit'
  fi
  git -C "$ROOT" fetch -q origin main
  if git -C "$ROOT" merge-base --is-ancestor "$head" origin/main; then pass "$(git -C "$ROOT" rev-parse --short HEAD) is on origin/main"; else
    problem 'HEAD is not pushed to origin/main'
  fi
  conclusion=$(gh run list -R "$REPOSITORY" --workflow ci.yml --commit "$head" --json status,conclusion \
    -q 'map(select(.status == "completed")) | .[0].conclusion // "missing"' 2>/dev/null || printf 'unknown')
  if [[ $conclusion == success ]]; then pass 'Ket CI passed for this commit'; else
    problem "Ket CI for this commit: $conclusion"
  fi
}

check_test_suite() {
  step 'Full test suite'
  if "$ROOT/scripts/test-all.sh" all >"$scratch/test-all.log" 2>&1; then
    pass "scripts/test-all.sh all ($(grep -c '^PASS' "$scratch/test-all.log") steps)"
  else
    problem "scripts/test-all.sh all failed; see $scratch/test-all.log"
    keep_scratch=true
  fi
}

# --- the real installer on this host ----------------------------------------------

check_installer() {
  step 'Installer and bootstrap (real DNS, real download)'
  local output head
  if output=$(bash "$ROOT/packaging/install-server.sh" --source-dir "$ROOT" --ref preflight --plan 2>&1) \
    && grep -Fq '=== Ket server plan ===' <<<"$output"; then
    pass 'the automatic-address plan works'
  else
    problem "the automatic-address plan failed: $output"
  fi
  if output=$(bash "$ROOT/packaging/install-server.sh" --source-dir "$ROOT" --ref preflight \
    unresolved.ket-preflight.invalid --plan 2>&1); then
    problem 'an unresolvable domain was accepted'
  elif grep -Fq 'does not resolve yet' <<<"$output"; then
    pass 'an unresolvable domain is explained'
  else
    problem "an unresolvable domain failed without the explanation: ${output:-no output}"
  fi
  head=$(git -C "$ROOT" rev-parse HEAD)
  if output=$(cd "$scratch" && KET_REF=$head TMPDIR=$scratch bash "$ROOT/install.sh" install --plan 2>&1) \
    && grep -Fq '=== Ket server plan ===' <<<"$output"; then
    pass 'the bootstrap downloads this commit and runs its installer'
  else
    problem "the bootstrap failed for this commit: $output"
  fi
}

# --- the canary server --------------------------------------------------------------

# masked_status CODE: true when an HTTP status is what any unknown path gets.
masked_status() { [[ $1 == 404 ]]; }

check_canary_release() {
  step "Canary $canary_target runs the candidate"
  local installed installed_commit head
  head=$(git -C "$ROOT" rev-parse HEAD)
  installed=$(canary sudo -n ket version 2>/dev/null) || { problem 'could not run "sudo ket version" on the canary'; return; }
  installed_commit=$(git -C "$ROOT" rev-parse -q --verify "$installed^{commit}" 2>/dev/null || printf '%s' "$installed")
  if [[ $installed_commit == "$head" ]]; then pass "the canary runs $installed"; else
    problem "the canary runs $installed, not this commit; deploy it: sudo ket update --to $head"
  fi
  if canary sudo -n ket doctor >"$scratch/doctor.log" 2>&1; then pass 'ket doctor finds no problems'; else
    problem "ket doctor found problems:$(grep -E 'FAIL' "$scratch/doctor.log" | sed 's/^/\n   /' | tr -d '\r')"
  fi
}

check_canary_exposure() {
  step 'Public exposure (through the public name, as scanners see it)'
  local public_url path code
  public_url=$(canary_public_url)
  [[ $public_url =~ ^https:// ]] || { problem "could not read the canary's KET_PUBLIC_URL"; return; }
  for path in "${PRIVATE_PATHS[@]}"; do
    code=$(curl -s -o /dev/null -m 20 -w '%{http_code}' "${public_url%/}$path" || true)
    if masked_status "$code"; then pass "$path is hidden ($code)"; else problem "$path answers $code from $public_url"; fi
  done
  code=$(curl -s -o /dev/null -m 20 -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
    --data '{"access_code":"preflight-preflight-preflight-00","client_name":"preflight"}' "${public_url%/}/v1/sessions" || true)
  if masked_status "$code"; then pass "a wrong access code is masked ($code)"; else problem "a wrong access code answers $code"; fi
}

check_canary_enrollment() {
  step 'Invite and enrollment with the shipped client code'
  local label output id version result
  label="release preflight $(git -C "$ROOT" rev-parse --short HEAD)"
  if ! output=$(canary_invite "$label" 1); then
    problem "ket invite failed on the canary: $output"
    return
  fi
  pass 'ket invite created a 30-minute, one-device invite'
  if ! (cd "$ROOT" && cargo build -q -p ket-client-core --example invite_enroll_check) 2>"$scratch/probe-build.log"; then
    problem "could not build the enrollment probe; see $scratch/probe-build.log"
  elif result=$(cd "$ROOT" && cargo run -q -p ket-client-core --example invite_enroll_check <<<"$output" 2>&1); then
    version=$(sed -n 's/^server_version=//p' <<<"$result")
    pass "enrolled, node signature verified, session released ($(sed -n 's/^transports=//p' <<<"$result"))"
    if grep -qx 'control=sealed' <<<"$result"; then pass 'the invite requires sealed control and every session call was sealed'; else
      problem 'the canary issued an invite without sealed control'
    fi
    if [[ $version == "$(workspace_version)" ]]; then pass "the canary reports version $version"; else
      problem "the canary reports version ${version:-none}, expected $(workspace_version)"
    fi
  else
    problem "enrollment failed: $result"
  fi
  if id=$(canary_revoke "$label"); then pass "the preflight invite $id is revoked"; else
    problem "could not revoke the preflight invite \"$label\"; revoke it with: sudo ket invites"
  fi
}

usage() {
  sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

main() {
  local mode=${1:-}
  [[ -n $mode ]] || { usage >&2; exit 2; }
  shift
  canary_target=''
  canary_ssh_options=()
  case $mode in
    local) ;;
    canary | all)
      [[ $# -ge 1 && $1 != -* ]] || die "$mode needs the canary's SSH target, such as ubuntu@203.0.113.10"
      canary_target=$1
      shift
      ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  while (($#)); do
    case $1 in
      --ssh-key) [[ $# -ge 2 ]] || die '--ssh-key needs a file'; canary_ssh_options+=(-i "$2" -o IdentitiesOnly=yes); shift 2 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  for tool in git gh jq curl cargo; do command -v "$tool" >/dev/null 2>&1 || die "$tool is required"; done

  scratch=$(mktemp -d "${TMPDIR:-/tmp}/ket-preflight.XXXXXX")
  keep_scratch=false
  trap '$keep_scratch || rm -rf -- "$scratch"' EXIT

  printf 'Ket release preflight for %s at %s\n' "v$(workspace_version)" "$(git -C "$ROOT" rev-parse --short HEAD)"
  if [[ $mode != canary ]]; then
    check_versions
    check_commit
    check_installer
    check_test_suite
  fi
  if [[ -n $canary_target ]]; then
    check_canary_release
    check_canary_exposure
    check_canary_enrollment
  fi

  printf '\n'
  if ((failures > 0)); then
    keep_scratch=true
    printf 'NOT READY: %d check(s) failed. Fix them, ship a new candidate commit, and rerun.\n' "$failures"
    exit 1
  fi
  if [[ $mode == all ]]; then
    printf 'READY: tag v%s on %s.\n' "$(workspace_version)" "$(git -C "$ROOT" rev-parse HEAD)"
  else
    printf 'All %s checks passed.\n' "$mode"
  fi
}

[[ -n ${RELEASE_PREFLIGHT_LIBRARY:-} ]] || main "$@"
