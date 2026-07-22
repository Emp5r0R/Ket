#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  printf 'Usage: %s LEAF_CERT CHAIN_CERT OUTPUT [TRUST_DIRECTORY]\n' "$0" >&2
  exit 1
fi

leaf=$1
chain=$2
output=$3
trust_dir=${4:-/etc/ssl/certs}
max_bytes=8192

for file in "$leaf" "$chain"; do
  [[ -r "$file" ]] || { printf 'Certificate file is not readable: %s\n' "$file" >&2; exit 1; }
done
[[ -d "$trust_dir" ]] || { printf 'Trust directory is unavailable: %s\n' "$trust_dir" >&2; exit 1; }

umask 077
temporary=$(mktemp "${output}.XXXXXX")
trap 'rm -f "$temporary"' EXIT
cp "$chain" "$temporary"

verify_bundle() {
  openssl verify -purpose sslserver -no-CApath -no-CAstore \
    -CAfile "$1" -untrusted "$chain" "$leaf" >/dev/null 2>&1
}

if ! verify_bundle "$temporary"; then
  anchor=
  shopt -s nullglob
  for candidate in "$trust_dir"/*.pem "$trust_dir"/*.crt; do
    [[ -r "$candidate" ]] || continue
    if openssl verify -purpose sslserver -no-CApath -no-CAstore \
      -CAfile "$candidate" -untrusted "$chain" "$leaf" >/dev/null 2>&1; then
      anchor=$candidate
      break
    fi
  done
  shopt -u nullglob
  [[ -n "$anchor" ]] || {
    printf 'No trusted root completes certificate chain for %s.\n' "$leaf" >&2
    exit 1
  }
  printf '\n' >>"$temporary"
  cat "$anchor" >>"$temporary"
fi

verify_bundle "$temporary" || {
  printf 'Generated certificate trust bundle does not verify %s.\n' "$leaf" >&2
  exit 1
}
size=$(wc -c <"$temporary")
(( size <= max_bytes )) || {
  printf 'Generated certificate trust bundle is %s bytes; maximum is %s.\n' "$size" "$max_bytes" >&2
  exit 1
}

mv -f "$temporary" "$output"
trap - EXIT
