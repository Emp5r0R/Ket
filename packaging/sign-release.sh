#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: KET_RELEASE_GPG_KEY=FINGERPRINT %s RELEASE_DIRECTORY\n' "$0"
}

[[ $# -eq 1 ]] || { usage >&2; exit 2; }
release_dir=$1
[[ -d $release_dir ]] || { printf 'Release directory does not exist: %s\n' "$release_dir" >&2; exit 1; }
[[ -n ${KET_RELEASE_GPG_KEY:-} ]] || { printf 'KET_RELEASE_GPG_KEY is required.\n' >&2; exit 1; }
command -v gpg >/dev/null 2>&1 || { printf 'gpg is required.\n' >&2; exit 127; }
command -v sha256sum >/dev/null 2>&1 || { printf 'sha256sum is required.\n' >&2; exit 127; }

if [[ -n ${KET_RELEASE_GPG_PASSPHRASE_FILE:-} ]]; then
  [[ -f $KET_RELEASE_GPG_PASSPHRASE_FILE && ! -L $KET_RELEASE_GPG_PASSPHRASE_FILE ]] \
    || { printf 'KET_RELEASE_GPG_PASSPHRASE_FILE must be a regular file.\n' >&2; exit 1; }
  gpg_secret_args=(--pinentry-mode loopback --passphrase-file "$KET_RELEASE_GPG_PASSPHRASE_FILE")
else
  gpg_secret_args=()
fi

mapfile -d '' artifacts < <(
  find "$release_dir" -maxdepth 1 -type f \
    \( -name '*.apk' -o -name '*.deb' -o -name '*.exe' \) \
    -printf '%f\0' | LC_ALL=C sort -z
)
((${#artifacts[@]} > 0)) || { printf 'No APK, DEB, or EXE artifacts found in %s.\n' "$release_dir" >&2; exit 1; }

declare -A seen=()
for artifact in "${artifacts[@]}"; do
  [[ $artifact != *$'\n'* && $artifact != *$'\r'* ]] \
    || { printf 'Artifact name contains a line break.\n' >&2; exit 1; }
  [[ -z ${seen[$artifact]:-} ]] || { printf 'Duplicate artifact name: %s\n' "$artifact" >&2; exit 1; }
  seen[$artifact]=1
done

checksum_tmp=$(mktemp "$release_dir/.SHA256SUMS.XXXXXX")
trap 'rm -f "$checksum_tmp"' EXIT
(
  cd "$release_dir"
  sha256sum -- "${artifacts[@]}"
) > "$checksum_tmp"
chmod 0644 "$checksum_tmp"
mv -f "$checksum_tmp" "$release_dir/SHA256SUMS"
trap - EXIT

sign() {
  local input=$1
  gpg --batch --yes --armor \
    --local-user "$KET_RELEASE_GPG_KEY" \
    "${gpg_secret_args[@]}" \
    --detach-sign --output "$input.asc" "$input"
  gpg --batch --verify "$input.asc" "$input" >/dev/null
}

for artifact in "${artifacts[@]}"; do
  sign "$release_dir/$artifact"
done
sign "$release_dir/SHA256SUMS"

gpg --batch --armor --export "$KET_RELEASE_GPG_KEY" \
  > "$release_dir/ket-release-signing-key.asc"
[[ -s $release_dir/ket-release-signing-key.asc ]] \
  || { printf 'Failed to export the release public key.\n' >&2; exit 1; }

printf 'Signed %d artifacts in %s\n' "${#artifacts[@]}" "$release_dir"
