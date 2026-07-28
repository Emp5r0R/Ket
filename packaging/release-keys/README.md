# Ket release key

Stable v1 artifacts use this OpenPGP signing identity:

```text
3FFD C479 EB97 4B59 5D70  C60F 754B 51EC 583C 5BFF
```

Import `ket-release-openpgp.asc`, inspect the full fingerprint, then verify `SHA256SUMS.asc` and each detached package signature. A matching signature proves artifact integrity and release-key ownership; it does not provide Windows Authenticode reputation.

The Android v1 signing certificate SHA-256 fingerprint is:

```text
A5279A2200E79BA8869B021DB75D844C92F4268048636A59C342803F1C84CB03
```
