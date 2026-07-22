#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
builder="$root/packaging/server/build-certificate-trust-bundle.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/ket-certificate-bundle.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/trust" "$work/empty-trust"

openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 2 \
  -subj '/CN=Ket Test Root' \
  -keyout "$work/root.key" -out "$work/root.crt" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -sha256 \
  -subj '/CN=Ket Test Intermediate' \
  -keyout "$work/intermediate.key" -out "$work/intermediate.csr" >/dev/null 2>&1
printf '%s\n' \
  'basicConstraints=critical,CA:TRUE,pathlen:0' \
  'keyUsage=critical,keyCertSign,cRLSign' >"$work/intermediate.ext"
openssl x509 -req -sha256 -days 2 \
  -in "$work/intermediate.csr" -CA "$work/root.crt" -CAkey "$work/root.key" \
  -CAcreateserial -extfile "$work/intermediate.ext" -out "$work/intermediate.crt" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -sha256 \
  -subj '/CN=ket.example.test' \
  -keyout "$work/leaf.key" -out "$work/leaf.csr" >/dev/null 2>&1
printf '%s\n' \
  'basicConstraints=critical,CA:FALSE' \
  'keyUsage=critical,digitalSignature,keyEncipherment' \
  'extendedKeyUsage=serverAuth' \
  'subjectAltName=DNS:ket.example.test' >"$work/leaf.ext"
openssl x509 -req -sha256 -days 2 \
  -in "$work/leaf.csr" -CA "$work/intermediate.crt" -CAkey "$work/intermediate.key" \
  -CAcreateserial -extfile "$work/leaf.ext" -out "$work/leaf.crt" >/dev/null 2>&1
cp "$work/root.crt" "$work/trust/ket-root.pem"

"$builder" "$work/leaf.crt" "$work/intermediate.crt" "$work/bundle.pem" "$work/trust"
openssl verify -purpose sslserver -no-CApath -no-CAstore -CAfile "$work/bundle.pem" \
  -untrusted "$work/intermediate.crt" "$work/leaf.crt" >/dev/null
[[ $(grep -c '^-----BEGIN CERTIFICATE-----$' "$work/bundle.pem") -eq 2 ]]

cat "$work/intermediate.crt" "$work/root.crt" >"$work/complete-chain.pem"
"$builder" "$work/leaf.crt" "$work/complete-chain.pem" "$work/complete-bundle.pem" "$work/empty-trust"
[[ $(grep -c '^-----BEGIN CERTIFICATE-----$' "$work/complete-bundle.pem") -eq 2 ]]

if "$builder" "$work/leaf.crt" "$work/intermediate.crt" "$work/missing.pem" "$work/empty-trust" >/dev/null 2>&1; then
  printf 'Trust bundle builder accepted a chain without a trusted root.\n' >&2
  exit 1
fi

printf 'Ket certificate trust bundle tests passed.\n'
