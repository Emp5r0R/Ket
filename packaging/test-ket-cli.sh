#!/usr/bin/env bash
# Tests for the server's ket command (packaging/server/ket.sh and cli/*.sh),
# loaded in library mode. Docker, systemd, the network, and root are replaced
# by stubs; everything happens in a temporary directory.
# Variables set here are read by the sourced libraries, stub functions are
# called by them (SC2317), and generated stub scripts are single-quoted on
# purpose so they expand when they run (SC2016).
# shellcheck disable=SC2034,SC2016,SC2317
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-cli.XXXXXX")
trap 'rm -rf "$work"' EXIT

fail_test() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}
equals() {
  [[ $2 == "$3" ]] || fail_test "$1: expected [$3], got [$2]"
}
contains() {
  [[ $2 == *"$3"* ]] || fail_test "$1: [$3] not found in: $2"
}

# Host locations all live in the sandbox.
export KET_SYSTEMD_DIR="$work/systemd" KET_BIN_DIR="$work/bin" KET_BACKUP_DIR="$work/backups"
export KET_STATE_DIR="$work/state" KET_LETSENCRYPT_DIR="$work/letsencrypt" KET_LOCK_FILE="$work/update.lock"
export KET_TTY="$work/tty"
mkdir -p "$KET_SYSTEMD_DIR" "$KET_BIN_DIR"
: > "$KET_TTY"

# shellcheck source=/dev/null
KET_CLI_LIBRARY=1 . "$root/packaging/server/ket.sh"
set -euo pipefail

# --- stubs ---------------------------------------------------------------------
log="$work/calls.log"
: > "$log"
require_root() { :; }
docker_cli() {
  printf 'docker %s\n' "$*" >> "$log"
  case "$1 ${2:-}" in
    'image inspect') [[ -f $work/images/${3##*:} ]] ;;
    'tag '*) mkdir -p "$work/images"; touch "$work/images/${3##*:}" ;;
    'rmi '*) rm -f "$work/images/${2##*:}" ;;
    'volume inspect') return 1 ;;
    *) return 0 ;;
  esac
}
eval "real_$(declare -f prepare_image)"
prepare_image() { printf 'prepare_image %s %s\n' "$2" "$3" >> "$log"; mkdir -p "$work/images"; touch "$work/images/$3"; }
ready=true
wait_for_ready() { $ready; }

# make_tree DIR REF: a minimal release tree whose scripts log their use.
make_tree() {
  local dir=$1
  mkdir -p "$dir/packaging/server"
  printf '#!/usr/bin/env bash\nexit "${KET_TEST_ENV_STATUS:-0}"\n' > "$dir/packaging/validate-env.sh"
  printf '#!/usr/bin/env bash\nprintf "compose %%s %%s\\n" "$(basename "$(cd "$(dirname "$0")/../.." && pwd)")" "$*" >> "%s"\n' "$log" \
    > "$dir/packaging/server/compose.sh"
  printf '#!/usr/bin/env bash\nprintf "create-invite %%s\\n" "$*"\n' > "$dir/packaging/server/create-invite.sh"
  printf '%s\n' "$2" > "$dir/RELEASE"
  chmod +x "$dir/packaging/validate-env.sh" "$dir/packaging/server/compose.sh" "$dir/packaging/server/create-invite.sh"
}

# make_install DIR REF: an installed tree with configuration and secrets.
make_install() {
  make_tree "$1" "$2"
  printf 'KET_ADMIN_TOKEN=old-token-0123456789012345678901\nKET_PUBLIC_URL=https://ket.example.test\nKET_CERTBOT_NAME=ket.example.test\n' > "$1/.env"
  chmod 0600 "$1/.env"
  mkdir -p "$1/secrets/tls"
  printf 'secret\n' > "$1/secrets/tls/privkey.pem"
  printf '%s\n' "$2" > "$1/.ket-release"
}

# --- release ratings (shared vectors with crates/ket-update) -------------------
vectors="$root/crates/ket-update/tests/vectors/server-updates.json"
vector_manifest=$(jq -c '.manifest' "$vectors")
cases=$(jq -c '.cases[]' "$vectors")
[[ -n $cases ]] || fail_test 'no shared update vectors'
while IFS= read -r case_json; do
  installed=$(jq -r '.installed' <<<"$case_json")
  target=$(jq -r '.target' <<<"$case_json")
  expected=$(jq -r '.category // ""' <<<"$case_json")
  equals "update rating $installed -> $target" "$(update_category "$vector_manifest" "$installed" "$target")" "$expected"
done <<<"$cases"

for allowed in all:optional all: security:security security:critical critical:critical; do
  auto_update_allows "${allowed%%:*}" "${allowed#*:}" || fail_test "KET_AUTO_UPDATE ${allowed%%:*} should apply ${allowed#*:}"
done
for refused in off:critical security:recommended security: critical:security; do
  if auto_update_allows "${refused%%:*}" "${refused#*:}"; then fail_test "KET_AUTO_UPDATE ${refused%%:*} applied ${refused#*:}"; fi
done
equals 'release tag version' "$(ref_version v1.10.2)" 1.10.2
equals 'non-release ref' "$(ref_version main || true)" ''
equals 'tag without v' "$(ref_version 1.2.3 || true)" ''
version_gt 1.10.0 1.9.9 || fail_test '1.10.0 should be newer than 1.9.9'
if version_gt 1.2.0 1.2.0; then fail_test 'equal versions are not newer'; fi

# --- server image: published when it matches, built otherwise -----------------------
(
  # Stub state uses names the code under test never declares (bash scoping is dynamic).
  pull_works=true stub_label=1.2.0
  docker_cli() {
    printf 'docker %s\n' "$*" >> "$log"
    case $1 in
      pull) $pull_works ;;
      image) printf '%s' "$stub_label" ;;
      *) return 0 ;;
    esac
  }
  : > "$log"
  real_prepare_image /tree v1.2.0 candidate >/dev/null
  grep -Fxq 'docker tag ghcr.io/emp5r0r/ket-control-plane:1.2.0 ket-control-plane:candidate' "$log" \
    || fail_test 'a matching published image was not used'
  grep -q '^docker build' "$log" && fail_test 'built although the published image matched'
  stub_label=1.1.0
  : > "$log"
  real_prepare_image /tree v1.2.0 candidate >/dev/null
  grep -Fxq 'docker build --tag ket-control-plane:candidate /tree' "$log" || fail_test 'a mismatched image was used'
  for case_args in 'main' 'v1.2.0 KET_BUILD_IMAGE'; do
    : > "$log"
    pull_works=true stub_label=1.2.0
    read -r -a case_words <<<"$case_args"
    set -- "${case_words[@]}"
    if [[ ${2:-} == KET_BUILD_IMAGE ]]; then
      KET_BUILD_IMAGE=1 real_prepare_image /tree "$1" local >/dev/null
    else
      real_prepare_image /tree "$1" local >/dev/null
    fi
    grep -q '^docker pull' "$log" && fail_test "pulled for: $case_args"
    grep -Fxq 'docker build --tag ket-control-plane:local /tree' "$log" || fail_test "did not build for: $case_args"
  done
  pull_works=false
  : > "$log"
  real_prepare_image /tree v1.2.0 local >/dev/null
  grep -Fxq 'docker build --tag ket-control-plane:local /tree' "$log" || fail_test 'a missing published image did not fall back to a build'
)

# --- .env helpers ----------------------------------------------------------------
install_dir="$work/env-test"
mkdir -p "$install_dir"
printf 'A=1\nKET_NODE_NAME="Ket Paris"\nB=2\nA=3\n' > "$install_dir/.env"
equals 'quoted value' "$(env_value KET_NODE_NAME)" 'Ket Paris'
equals 'last value wins' "$(env_value A)" 3
set_env_value A new
equals 'replaced value' "$(grep -c '^A=' "$install_dir/.env")" 1
equals 'replacement' "$(env_value A)" new
set_env_value C added
equals 'appended value' "$(env_value C)" added
equals 'env file mode' "$(stat -c %a "$install_dir/.env")" 600
equals 'public host' "$(printf 'KET_PUBLIC_URL=https://vpn.example.com:8443/\n' > "$install_dir/.env"; public_host)" vpn.example.com
# The control plane is reached on the installation's KET_CONTROL_PORT (some
# hand-made v1.0 servers publish it on another port), never a fixed 8787.
printf 'KET_CONTROL_PORT=18787\n' > "$install_dir/.env"
equals 'control port from .env' "$(control_url /readyz)" http://127.0.0.1:18787/readyz
: > "$install_dir/.env"
equals 'default control port' "$(control_url /readyz)" http://127.0.0.1:8787/readyz
mkdir -p "$work/other-tree"
printf 'KET_CONTROL_PORT=9000\n' > "$work/other-tree/.env"
equals 'control port of another tree' "$(control_url /metrics "$work/other-tree")" http://127.0.0.1:9000/metrics
if grep -n '127\.0\.0\.1:8787' "$root"/packaging/server/cli/*.sh; then
  fail_test 'a ket command calls the control plane on a fixed port'
fi

# --- prompts -------------------------------------------------------------------------
printf 'Family\n' > "$KET_TTY"
equals 'typed answer' "$(ask 'Who' 'Ket devices')" Family
: > "$KET_TTY"
equals 'default answer' "$(ask 'Who' 'Ket devices')" 'Ket devices'
printf 'yes\n' > "$KET_TTY"
confirm 'Go?' || fail_test 'yes should confirm'
printf 'nope\n' > "$KET_TTY"
if confirm 'Go?'; then fail_test 'only yes confirms'; fi
has_terminal || fail_test 'the test terminal should be usable'
if KET_TTY="$work/no-terminal" has_terminal; then fail_test 'a missing terminal was usable'; fi

# --- invite wizard ---------------------------------------------------------------------
install_dir="$work/invite-install"
make_install "$install_dir" v1.1.0
(
  # Scripted answers; ask runs in command substitutions, so the queue lives in files.
  answers() { printf '%s\n' "$@" > "$work/answers"; printf '0' > "$work/answer-index"; }
  ask() {
    local index
    index=$(cat "$work/answer-index")
    printf '%s' "$((index + 1))" > "$work/answer-index"
    sed -n "$((index + 1))p" "$work/answers"
  }
  answers 'Family phones' 3 7
  output=$(cmd_invite)
  contains 'wizard runs create-invite with the answers' "$output" 'create-invite --label Family phones --devices 3 --days 7'
  answers 0 abc 9999
  if (ask_number 'Devices' 1 512 >/dev/null 2>&1); then fail_test 'invalid device counts were accepted'; fi
  answers 0 4
  equals 'number re-asked until valid' "$(ask_number 'Devices' 1 512)" 4
  output=$(cmd_invite --devices 2 --days 30)
  contains 'options skip the wizard' "$output" 'create-invite --devices 2 --days 30'
)
if (cmd_revoke 'bad id!' 2>/dev/null); then fail_test 'an unsafe grant ID was accepted'; fi

# --- node keys (shared vectors with crates/ket-invite) ------------------------------------
invite_vectors="$root/crates/ket-invite/tests/vectors/invite-v1.json"
seed=$(hex_to_binary "$(jq -r '.node_seed_hex' "$invite_vectors")" | base64 | tr -d '\n=' | tr '+/' '-_')
public=$(node_public_key_hex "$seed")
equals 'node public key' "$public" "$(jq -r '.valid[0].node_public_key_hex' "$invite_vectors")"
equals 'node fingerprint' "$(node_fingerprint "$public")" "$(jq -r '.valid[0].node_fingerprint' "$invite_vectors")"

install_dir="$work/keys-install"
make_install "$install_dir" v1.1.0
restart_control_plane() { printf 'restart\n' >> "$log"; }
rotate_admin_token >/dev/null
[[ $(env_value KET_ADMIN_TOKEN) != old-token-0123456789012345678901 ]] || fail_test 'admin token was not replaced'
new_token=$(env_value KET_ADMIN_TOKEN)
((${#new_token} >= 32)) || fail_test 'new admin token is too short'
: > "$KET_TTY"
output=$(rotate_node_key)
contains 'node key added' "$output" 'New node fingerprint:'
first_key=$(env_value KET_NODE_SIGNING_KEY)
printf 'no\n' > "$KET_TTY"
output=$(rotate_node_key 2>&1)
contains 'existing node key needs typed confirmation' "$output" 'Nothing changed.'
equals 'node key kept' "$(env_value KET_NODE_SIGNING_KEY)" "$first_key"

# --- host integration -------------------------------------------------------------------
install_dir=/opt/ket-test
install_host_integration
for unit in ket-certificates.service ket-certificates.timer ket-update.service ket-update.timer; do
  [[ -f $KET_SYSTEMD_DIR/$unit ]] || fail_test "$unit was not written"
done
grep -Fqx 'ExecStart=/opt/ket-test/packaging/server/ket.sh update --auto' "$KET_SYSTEMD_DIR/ket-update.service" \
  || fail_test 'update service does not run the installed ket command'
equals 'ket command link' "$(readlink "$KET_BIN_DIR/ket")" /opt/ket-test/packaging/server/ket.sh
remove_host_integration
[[ -z $(ls -A "$KET_SYSTEMD_DIR") && ! -e $KET_BIN_DIR/ket ]] || fail_test 'host integration was not removed'
mkdir -p "$KET_LETSENCRYPT_DIR/renewal-hooks/deploy"
: > "$KET_LETSENCRYPT_DIR/renewal-hooks/deploy/ket-server"
install_host_integration
[[ ! -e $KET_SYSTEMD_DIR/ket-certificates.timer ]] || fail_test 'a v1.0.1 host-certbot install got a second renewal timer'
[[ -e $KET_SYSTEMD_DIR/ket-update.timer ]] || fail_test 'a v1.0.1 install did not get the update timer'
remove_host_integration
[[ ! -e $KET_LETSENCRYPT_DIR/renewal-hooks/deploy/ket-server ]] || fail_test 'the legacy renewal hook was left behind'

# --- status helpers ---------------------------------------------------------------------
(
  compose() { printf '[{"Service":"control-plane","State":"running","Health":"healthy"},{"Service":"xray","State":"exited","Health":""}]\n'; }
  equals 'service states (array)' "$(service_states | tr '\n' ';')" 'control-plane running healthy;xray exited -;'
  compose() { printf '{"Service":"edge-proxy","State":"running","Health":""}\n{"Service":"hysteria2","State":"restarting","Health":""}\n'; }
  equals 'service states (lines)' "$(service_states | tr '\n' ';')" 'edge-proxy running -;hysteria2 restarting -;'
)
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 10 -subj /CN=test \
  -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
days=$(certificate_days_left "$work/cert.pem")
((days == 9 || days == 10)) || fail_test "certificate days left: $days"
if certificate_days_left "$work/missing.pem" >/dev/null; then fail_test 'a missing certificate had days left'; fi

# --- update decisions ----------------------------------------------------------------------
install_dir="$work/decide-install"
make_install "$install_dir" v1.0.1
printf '#!/usr/bin/env bash\nprintf "BOOTSTRAP %%s %%s %%s\\n" "$1" "$KET_REF" "$KET_INSTALL_DIR"\n' > "$install_dir/install.sh"
latest_release_tag() { printf 'v1.2.0'; }
release_manifest() { printf '%s' "$vector_manifest"; }
output=$(cmd_update --check)
contains 'check reports the rating' "$output" 'Ket v1.2.0 is available (installed: v1.0.1, update rating: security)'
equals 'check is recorded' "$(jq -r .outcome "$KET_STATE_DIR/update-check.json")" available
printf 'KET_AUTO_UPDATE=critical\n' >> "$install_dir/.env"
output=$(cmd_update --auto)
contains 'auto waits below its policy' "$output" 'Not applied automatically (KET_AUTO_UPDATE=critical)'
equals 'waiting is recorded' "$(jq -r .outcome "$KET_STATE_DIR/update-check.json")" 'waiting: KET_AUTO_UPDATE=critical'
set_env_value KET_AUTO_UPDATE security
output=$(cmd_update --auto)
equals 'auto hands over to the bootstrap' "$output" "$(printf 'Ket v1.2.0 is available (installed: v1.0.1, update rating: security).\nBOOTSTRAP update v1.2.0 %s' "$install_dir")"
output=$(cmd_update --yes --to v1.1.0)
contains 'explicit target' "$output" 'BOOTSTRAP update v1.1.0'
printf 'no\n' > "$KET_TTY"
output=$(cmd_update)
contains 'declined update changes nothing' "$output" 'Nothing changed.'
latest_release_tag() { printf 'v1.0.1'; }
output=$(cmd_update)
contains 'up to date' "$output" 'Ket v1.0.1 is up to date.'
latest_release_tag() { printf 'v1.0.0'; }
output=$(cmd_update)
contains 'never downgrades on its own' "$output" 'newer than the latest release'
printf 'source\n' > "$install_dir/.ket-release"
latest_release_tag() { printf 'v1.2.0'; }
output=$(cmd_update --auto)
contains 'checkout installs are not auto-updated' "$output" 'Automatic updates need a release install'

# --- applying an update ----------------------------------------------------------------------
(
  install_dir="$work/opt/ket"
  make_install "$install_dir" v1.0.1
  release="$work/download"
  make_tree "$release" v1.2.0
  mkdir -p "$work/images"; touch "$work/images/local"
  create_backup() { printf '%s/ket-test-%s.tar.gz' "$KET_BACKUP_DIR" "$1"; }
  prune_backups() { :; }
  install_host_integration() { printf 'host-integration %s\n' "$install_dir" >> "$log"; }
  : > "$log"
  output=$(cmd_apply_update --source-dir "$release" --ref v1.2.0 --install-dir "$install_dir" --remove-source)
  contains 'update succeeds' "$output" 'Ket is now v1.2.0.'
  equals 'new release installed' "$(cat "$install_dir/RELEASE")" v1.2.0
  equals 'release marker' "$(installed_ref)" v1.2.0
  equals 'previous release kept' "$(cat "$install_dir.previous/RELEASE")" v1.0.1
  equals 'settings carried over' "$(env_value KET_ADMIN_TOKEN)" old-token-0123456789012345678901
  [[ -n $(env_value KET_NODE_SIGNING_KEY) ]] || fail_test 'migration did not add a node signing key'
  equals 'secrets carried over' "$(cat "$install_dir/secrets/tls/privkey.pem")" secret
  [[ ! -e $release ]] || fail_test 'the downloaded release was not removed'
  [[ -z $(find "$work/opt" -maxdepth 1 -name '.ket-update.*') ]] || fail_test 'a staging directory was left behind'
  grep -Fq 'prepare_image v1.2.0 candidate' "$log" || fail_test 'the new image was not prepared before the swap'
  grep -Fq 'compose ket up --detach --no-build --remove-orphans' "$log" || fail_test 'the new release was not started'
  grep -Fq "host-integration $install_dir" "$log" || fail_test 'host integration was not refreshed'
  [[ -f $work/images/local && -f $work/images/previous && ! -f $work/images/candidate ]] \
    || fail_test 'image tags were not rotated'
  equals 'success is recorded' "$(jq -r .outcome "$KET_STATE_DIR/update-check.json")" 'updated from v1.0.1'

  # Rollback swaps the current and previous releases, both ways.
  output=$(cmd_rollback --yes)
  contains 'rollback' "$output" 'Ket is now v1.0.1.'
  equals 'rolled back' "$(cat "$install_dir/RELEASE")" v1.0.1
  equals 'newer release kept for roll-forward' "$(cat "$install_dir.previous/RELEASE")" v1.2.0
  # Each command holds the update lock until it exits, so each runs in its own process.
  output=$(cmd_rollback --yes)
  equals 'rolled forward' "$(cat "$install_dir/RELEASE")" v1.2.0
  : > "$KET_TTY"
  output=$(cmd_rollback)
  contains 'rollback asks first' "$output" 'Nothing changed.'

  # A release that does not become ready is replaced by the one before it.
  make_tree "$release" v1.3.0
  ready=false
  if output=$(cmd_apply_update --source-dir "$release" --ref v1.3.0 --install-dir "$install_dir" 2>&1); then
    fail_test 'an unready release was accepted'
  fi
  contains 'failure explains the rollback' "$output" 'the update failed and Ket v1.2.0 was restored'
  equals 'previous release restored' "$(cat "$install_dir/RELEASE")" v1.2.0
  failed=$(find "$work/opt" -maxdepth 1 -name 'ket.failed-*' | head -n 1)
  [[ -n $failed && -f $failed/update-failure.log ]] || fail_test 'the failed release was not kept with its logs'
  equals 'failed tree is the new release' "$(cat "$failed/RELEASE")" v1.3.0
  ready=true

  # A configuration the new release rejects changes nothing.
  make_tree "$release" v1.4.0
  if output=$(KET_TEST_ENV_STATUS=1 cmd_apply_update --source-dir "$release" --ref v1.4.0 --install-dir "$install_dir" 2>&1); then
    fail_test 'a rejected configuration was applied'
  fi
  contains 'validation failure' "$output" 'rejects this server configuration; nothing was changed'
  equals 'installation untouched' "$(cat "$install_dir/RELEASE")" v1.2.0
  [[ -z $(find "$work/opt" -maxdepth 1 -name '.ket-update.*') ]] || fail_test 'a rejected update left staging behind'
)

# --- uninstall ----------------------------------------------------------------------
install_dir="$work/remove/ket"
make_install "$install_dir" v1.2.0
mkdir -p "$install_dir.previous"
install_host_integration
mkdir -p "$KET_LETSENCRYPT_DIR/live/ket.example.test"
KET_TTY="$work/no-terminal"
if (cmd_uninstall >/dev/null 2>&1); then fail_test 'uninstall ran without confirmation'; fi
[[ -d $install_dir ]] || fail_test 'an unconfirmed uninstall removed files'
KET_TTY="$work/tty"
printf 'remove\n' > "$KET_TTY"
output=$(cmd_uninstall)
contains 'typed confirmation required' "$output" 'Nothing changed.'
: > "$log"
output=$(cmd_uninstall --yes)
contains 'uninstall' "$output" 'Ket is removed.'
[[ ! -e $install_dir && ! -e $install_dir.previous ]] || fail_test 'installation directories remain'
[[ -z $(ls -A "$KET_SYSTEMD_DIR") && ! -e $KET_BIN_DIR/ket ]] || fail_test 'timers or the ket command remain'
grep -Fq 'compose ket down --volumes --remove-orphans' "$log" || fail_test 'containers and volumes were not removed'
backup=$(find "$KET_BACKUP_DIR" -name 'ket-*-before-uninstall.tar.gz' | head -n 1)
[[ -n $backup ]] || fail_test 'no final backup was written'
tar -tzf "$backup" | grep -Fqx './.env' || fail_test 'the final backup lacks .env'
tar -tzf "$backup" | grep -Fqx './secrets/tls/privkey.pem' || fail_test 'the final backup lacks secrets'
equals 'backup mode' "$(stat -c %a "$backup")" 600
[[ -d $KET_LETSENCRYPT_DIR/live/ket.example.test ]] || fail_test 'a plain uninstall removed the certificate'

make_install "$install_dir" v1.2.0
output=$(cmd_uninstall --purge --yes)
[[ ! -e $KET_BACKUP_DIR && ! -e $KET_LETSENCRYPT_DIR/live/ket.example.test ]] \
  || fail_test 'purge left backups or the certificate'

# --- the command itself ---------------------------------------------------------------
output=$(bash "$root/packaging/server/ket.sh" help)
contains 'help lists commands' "$output" 'invite              Create an invite'
if bash "$root/packaging/server/ket.sh" frobnicate >/dev/null 2>&1; then fail_test 'an unknown command succeeded'; fi
mkdir -p "$work/version"
printf 'v9.8.7\n' > "$work/version/.ket-release"
equals 'version' "$(KET_INSTALL_DIR="$work/version" bash "$root/packaging/server/ket.sh" version)" v9.8.7
ln -s "$root/packaging/server/ket.sh" "$work/bin/ket-link"
contains 'runs through a symlink' "$(bash "$work/bin/ket-link" help)" 'Manage this Ket server.'

printf 'Ket server command tests passed.\n'
