# Release signing

Ket v1 releases use two persistent identities:

- Android APKs are signed by the owner-held PKCS#12 key. The build fails unless the resulting certificate SHA-256 fingerprint matches `KET_ANDROID_CERT_SHA256`.
- Every APK, DEB, and EXE plus `SHA256SUMS` receives an armored detached OpenPGP signature. The release includes `ket-release-signing-key.asc`.

The Windows EXE does not have a commercial Authenticode certificate. OpenPGP verifies that the published bytes came from the Ket release key, but Windows will still show an unknown-publisher warning. Do not describe that package as Authenticode-signed.

Private material belongs on a filesystem that enforces Unix permissions. The v1 owner keys on the release workstation are under `~/.ket-release/`, mode `0700`; secret files are mode `0600`. Never commit or upload the keystore, secret keyring, or passphrases. Do not place keys on the project filesystem when its mount ignores `chmod`.

## Android build

Load the owner-held values, then build and verify the release variant:

```bash
set -a
. "$HOME/.ket-release/release.env"
set +a
./packaging/build-android.sh release
```

## Package signatures

Place only final APK, DEB, and EXE artifacts in one directory, then run:

```bash
set -a
. "$HOME/.ket-release/release.env"
set +a
./packaging/sign-release.sh release-artifacts/v1.0.1
```

The signer regenerates `SHA256SUMS`, signs every package and the checksum file, verifies each new signature, and exports the public key.

## User verification

```bash
gpg --import ket-release-signing-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check SHA256SUMS
gpg --verify Ket_1.0.1_amd64.deb.asc Ket_1.0.1_amd64.deb
```

Compare the imported fingerprint with `3FFD C479 EB97 4B59 5D70 C60F 754B 51EC 583C 5BFF`, which is also tracked under `packaging/release-keys/README.md`, before trusting the key.
