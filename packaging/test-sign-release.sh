#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export GNUPGHOME="$scratch/gnupg"
mkdir -m 0700 "$GNUPGHOME"

gpg --batch --passphrase '' --quick-generate-key \
  'Ket Release Test <release-test@ket.invalid>' ed25519 sign 1d >/dev/null 2>&1
fingerprint=$(gpg --batch --with-colons --list-secret-keys \
  'release-test@ket.invalid' | awk -F: '$1 == "fpr" { print $10; exit }')
[[ ${#fingerprint} -eq 40 || ${#fingerprint} -eq 64 ]]

release_dir="$scratch/artifacts"
mkdir "$release_dir"
printf 'android\n' > "$release_dir/Ket_1.0.0_android.apk"
printf 'linux\n' > "$release_dir/Ket_1.0.0_amd64.deb"
printf 'windows\n' > "$release_dir/Ket_1.0.0_x64-setup.exe"

KET_RELEASE_GPG_KEY="$fingerprint" "$root/packaging/sign-release.sh" "$release_dir"

(
  cd "$release_dir"
  sha256sum --check SHA256SUMS >/dev/null
  gpg --batch --verify SHA256SUMS.asc SHA256SUMS >/dev/null 2>&1
  for artifact in *.apk *.deb *.exe; do
    gpg --batch --verify "$artifact.asc" "$artifact" >/dev/null 2>&1
  done
)
grep -q 'BEGIN PGP PUBLIC KEY BLOCK' "$release_dir/ket-release-signing-key.asc"

empty="$scratch/empty"
mkdir "$empty"
if KET_RELEASE_GPG_KEY="$fingerprint" "$root/packaging/sign-release.sh" "$empty" >/dev/null 2>&1; then
  printf 'Signing unexpectedly accepted an empty artifact directory.\n' >&2
  exit 1
fi

printf 'Release signing tests passed.\n'
