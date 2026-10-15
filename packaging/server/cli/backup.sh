# shellcheck shell=bash
# Backups of an installation's configuration, keys, and state. Sourced by the
# ket command; never executed.
# shellcheck disable=SC2034,SC2154

readonly KET_STATE_VOLUME=ket_ket-state

# create_backup LABEL: writes one archive with .env, secrets/, the release
# marker, and the grants/sessions state volume, and prints its path.
create_backup() {
  local label=$1 dir archive
  dir=$(ket_backup_dir)
  install -d -m 0700 "$dir"
  archive="$dir/ket-$(timestamp)-$label.tar.gz"
  (
    # errexit does not apply inside a checked subshell, so every step is checked.
    work=$(mktemp -d) || exit 1
    trap 'rm -rf -- "$work"' EXIT
    cp -a "$install_dir/.env" "$work/.env" || exit 1
    if [[ -d $install_dir/secrets ]]; then cp -a "$install_dir/secrets" "$work/secrets" || exit 1; fi
    printf '%s\n' "$(installed_ref)" > "$work/.ket-release" || exit 1
    if docker_cli volume inspect "$KET_STATE_VOLUME" >/dev/null 2>&1; then
      docker_cli run --rm --user 0:0 --entrypoint tar \
        --volume "$KET_STATE_VOLUME:/state:ro" "$KET_IMAGE:local" \
        -C /state -cf - . > "$work/state.tar" || exit 1
    fi
    umask 077
    tar -C "$work" -czf "$archive" . || exit 1
  ) || { rm -f -- "$archive"; fail 'could not write the backup'; }
  chmod 0600 "$archive"
  printf '%s' "$archive"
}

# Keeps the newest KET_BACKUP_KEEP (default 10) archives.
prune_backups() {
  local keep=${KET_BACKUP_KEEP:-10} dir
  dir=$(ket_backup_dir)
  [[ -d $dir ]] || return 0
  find "$dir" -maxdepth 1 -type f -name 'ket-*.tar.gz' -printf '%T@ %p\n' \
    | sort -rn | tail -n +"$((keep + 1))" | cut -d' ' -f2- | xargs -r rm -f --
}

cmd_backup() {
  require_root
  require_installation
  local archive
  archive=$(create_backup manual)
  prune_backups
  info "Backup written to $archive"
  info 'It holds your keys and secrets: keep it somewhere safe.'
}
