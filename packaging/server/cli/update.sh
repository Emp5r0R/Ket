# shellcheck shell=bash
# Server updates: deciding (ket update), applying a downloaded release with a
# health-checked swap (ket.sh apply-update, run from the NEW release), and
# going back (ket rollback). Sourced by the ket command; never executed.
# shellcheck disable=SC2034,SC2154

# version_gt A B: true when MAJOR.MINOR.PATCH A is newer than B.
version_gt() {
  [[ $1 != "$2" && "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" == "$1" ]]
}

update_usage() {
  cat <<'USAGE'
Usage: ket update [--check] [--to TAG] [--yes]

  --check     Only report whether a newer release exists and how urgent it is.
  --to TAG    Update to this release instead of the latest one.
  --yes       Do not ask for confirmation.
  --auto      Used by the daily timer: apply the update only when
              KET_AUTO_UPDATE in .env allows its rating (default: security).
USAGE
}

cmd_update() {
  local check=false auto=false yes=false target='' explicit=false
  while (($#)); do
    case $1 in
      --check) check=true; shift ;;
      --auto) auto=true; yes=true; shift ;;
      --yes|-y) yes=true; shift ;;
      --to) [[ $# -ge 2 ]] || fail '--to needs a release tag'; target=$2; explicit=true; shift 2 ;;
      -h|--help) update_usage; return 0 ;;
      *) update_usage >&2; fail "unknown option: $1" ;;
    esac
  done
  require_root
  require_installation
  require_commands curl jq
  [[ -z $target || $target =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || fail "unexpected release name: $target"

  local installed installed_version target_version manifest category='' rating
  installed=$(installed_ref)
  if [[ -z $target ]]; then
    target=$(latest_release_tag) || fail 'could not look up the latest Ket release; check outbound HTTPS to github.com'
  fi
  installed_version=$(ref_version "$installed" || true)
  target_version=$(ref_version "$target" || true)
  if [[ -n $installed_version && -n $target_version ]] \
    && manifest=$(release_manifest "$target" 2>/dev/null); then
    category=$(update_category "$manifest" "$installed_version" "$target_version")
  fi
  rating=${category:-unrated}

  if [[ $installed == "$target" ]]; then
    info "Ket $installed is up to date."
    record_update_check "$installed" "$target" '' current
    return 0
  fi
  if [[ -n $installed_version && -n $target_version ]] && ! version_gt "$target_version" "$installed_version" && ! $explicit; then
    info "Ket $installed is newer than the latest release ($target); nothing to do."
    record_update_check "$installed" "$target" '' current
    return 0
  fi
  info "Ket $target is available (installed: $installed, update rating: $rating)."
  if $check; then
    record_update_check "$installed" "$target" "$category" available
    return 0
  fi
  if $auto; then
    local policy
    policy=$(auto_update_policy)
    if [[ -z $installed_version ]]; then
      info "Automatic updates need a release install; this one runs $installed. Update with: sudo ket update"
      record_update_check "$installed" "$target" "$category" 'waiting: not a release install'
      return 0
    fi
    if ! auto_update_allows "$policy" "$category"; then
      info "Not applied automatically (KET_AUTO_UPDATE=$policy). Update with: sudo ket update"
      record_update_check "$installed" "$target" "$category" "waiting: KET_AUTO_UPDATE=$policy"
      return 0
    fi
  elif ! $yes; then
    has_terminal || fail 'no terminal to confirm on; rerun with --yes'
    confirm "Update Ket $installed to $target?" || { info 'Nothing changed.'; return 0; }
  fi
  record_update_check "$installed" "$target" "$category" applying
  # The current release's bootstrap downloads the target and runs the target's
  # own apply-update, so the newer release decides how it is installed.
  exec env "KET_REF=$target" "KET_INSTALL_DIR=$install_dir" bash "$install_dir/install.sh" update
}

# --- applying a release (runs from the downloaded release) --------------------

acquire_update_lock() {
  require_commands flock
  exec {update_lock_fd}>"${KET_LOCK_FILE:-/run/ket-update.lock}"
  flock -n "$update_lock_fd" || fail 'another Ket update is already running'
}

# Moves operator configuration into the new tree: .env, secrets/, and any
# Compose override file. Nothing else in the old tree is kept.
carry_configuration() {
  local from=$1 to=$2 file
  cp -a "$from/.env" "$to/.env"
  [[ ! -d $from/secrets ]] || cp -a "$from/secrets" "$to/"
  for file in compose.override.yaml compose.override.yml; do
    [[ ! -f $from/$file ]] || cp -a "$from/$file" "$to/$file"
  done
}

# Brings an older .env up to what this release expects, without changing any
# existing value.
migrate_environment() {
  local env_file=$1
  if [[ -z $(env_value KET_NODE_SIGNING_KEY "$env_file") ]]; then
    set_env_value KET_NODE_SIGNING_KEY \
      "$(openssl rand 32 | base64 | tr -d '\n=' | tr '+/' '-_')" "$env_file"
    info 'Added a node signing key: this server now also issues invites.'
  fi
}

validate_tree() {
  validate_configuration "$1" || fail 'the new release rejects this server configuration; nothing was changed'
  compose_in "$1" config --quiet || fail 'the new release has an invalid Compose configuration; nothing was changed'
}

# Waits until the control plane is ready and the public edge answers locally.
wait_for_ready() {
  local attempts=${KET_READY_ATTEMPTS:-90} host attempt
  host=$(public_host)
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if curl --fail --silent --max-time 5 "$(control_url /readyz)" >/dev/null 2>&1 \
      && curl --fail --silent --max-time 5 --resolve "$host:443:127.0.0.1" "https://$host/" >/dev/null 2>&1; then
      return 0
    fi
    ((attempt < attempts)) && sleep 2
  done
  return 1
}

start_and_wait() {
  compose up --detach --no-build --remove-orphans && wait_for_ready
}

# Swaps two image tags of the server image (either may be missing).
swap_image_tags() {
  local first=$1 second=$2
  if image_exists "$first"; then docker_cli tag "$KET_IMAGE:$first" "$KET_IMAGE:swap"; fi
  if image_exists "$second"; then docker_cli tag "$KET_IMAGE:$second" "$KET_IMAGE:$first"; fi
  if image_exists swap; then
    docker_cli tag "$KET_IMAGE:swap" "$KET_IMAGE:$second"
    docker_cli rmi "$KET_IMAGE:swap" >/dev/null
  fi
}

apply_usage() {
  printf 'Usage: ket.sh apply-update --source-dir DIR --ref TAG [--install-dir DIR] [--remove-source]\n'
}

cmd_apply_update() {
  local ref='' remove_source=false
  while (($#)); do
    case $1 in
      --source-dir) [[ $# -ge 2 ]] || fail '--source-dir needs a value'; source_dir=$2; shift 2 ;;
      --ref) [[ $# -ge 2 ]] || fail '--ref needs a value'; ref=$2; shift 2 ;;
      --install-dir) [[ $# -ge 2 ]] || fail '--install-dir needs a value'; install_dir=$2; shift 2 ;;
      --remove-source) remove_source=true; shift ;;
      *) apply_usage >&2; fail "unknown option: $1" ;;
    esac
  done
  [[ -n $ref && -d $source_dir/packaging ]] || { apply_usage >&2; fail 'a release tree and its tag are required'; }
  require_root
  require_installation
  require_commands curl jq tar openssl
  acquire_update_lock

  local from staging previous backup
  from=$(installed_ref)
  previous="$install_dir.previous"
  info "Updating Ket $from to $ref..."
  staging=$(mktemp -d "$(dirname -- "$install_dir")/.ket-update.XXXXXX")
  # shellcheck disable=SC2064
  trap "rm -rf -- '$staging'" EXIT
  tar -C "$source_dir" --exclude=./.git -cf - . | tar -C "$staging" -xf -
  carry_configuration "$install_dir" "$staging"
  migrate_environment "$staging/.env"
  printf '%s\n' "$ref" > "$staging/.ket-release"
  validate_tree "$staging"
  prepare_image "$staging" "$ref" candidate
  backup=$(create_backup "before-$ref")
  prune_backups
  info "Backup written to $backup"

  rm -rf -- "$previous"
  mv -- "$install_dir" "$previous"
  mv -- "$staging" "$install_dir"
  trap - EXIT
  swap_image_tags local previous
  swap_image_tags local candidate
  docker_cli rmi "$KET_IMAGE:candidate" >/dev/null 2>&1 || true
  $remove_source && rm -rf -- "$source_dir"

  if start_and_wait; then
    install_host_integration
    record_update_check "$ref" "$ref" '' "updated from $from"
    info "Ket is now $ref."
    info "The previous release is kept in $previous. To go back: sudo ket rollback"
    return 0
  fi
  restore_previous "$from" "$backup"
}

# Puts the previous release back after a failed update and exits non-zero.
restore_previous() {
  local from=$1 backup=$2 failed
  failed="$install_dir.failed-$(timestamp)"
  warn "the new release did not become ready; restoring Ket $from"
  compose logs --no-color --tail=200 > "$install_dir/update-failure.log" 2>&1 || true
  mv -- "$install_dir" "$failed"
  mv -- "$install_dir.previous" "$install_dir"
  swap_image_tags local previous
  if ! start_and_wait; then
    warn "Ket $from did not become ready either. Your backup is $backup; see: sudo ket doctor"
  fi
  record_update_check "$from" "$(installed_ref "$failed")" '' 'failed and rolled back'
  fail "the update failed and Ket $from was restored. Logs: $failed/update-failure.log"
}

cmd_rollback() {
  local yes=false previous from to parked
  case ${1:-} in
    --yes|-y) yes=true ;;
    '') ;;
    *) fail 'usage: ket rollback [--yes]' ;;
  esac
  require_root
  require_installation
  acquire_update_lock
  previous="$install_dir.previous"
  [[ -f $previous/.env ]] || fail 'there is no previous release to go back to'
  from=$(installed_ref)
  to=$(installed_ref "$previous")
  info "This switches Ket $from back to $to. A release that changed the stored state may not read it any more; a backup is taken first."
  if ! $yes; then
    has_terminal || fail 'no terminal to confirm on; rerun with --yes'
    confirm "Go back to Ket $to?" || { info 'Nothing changed.'; return 0; }
  fi
  info "Backup written to $(create_backup "before-rollback")"
  parked="$install_dir.next"
  rm -rf -- "$parked"
  mv -- "$install_dir" "$parked"
  mv -- "$previous" "$install_dir"
  mv -- "$parked" "$previous"
  swap_image_tags local previous
  start_and_wait || fail "Ket $to did not become ready; see: sudo ket doctor"
  install_host_integration
  record_update_check "$to" "$from" '' "rolled back from $from"
  info "Ket is now $to. To return to $from: sudo ket rollback"
}
