#!/usr/bin/env bash
# Let's Encrypt certificates for Ket, using the pinned certbot container so the
# host needs no certbot package.
#
#   certificates.sh issue NAME [EXTRA_NAME] [EMAIL]   (installer)
#   certificates.sh renew                              (daily timer)
#
# renew refreshes Ket's copies and restarts TLS services only when the
# certificate actually changed.
set -euo pipefail

readonly CERTBOT_IMAGE='certbot/certbot:v5.8.0@sha256:f70ad0adbb7e117f0fe42a63c553f28ea451edabc0148757b6efcd9735acaa20'

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)

fail() {
  printf 'ket certificates: %s\n' "$1" >&2
  exit 1
}

certbot() {
  install -d -m 0755 /etc/letsencrypt /var/lib/letsencrypt
  docker run --rm --name ket-certbot \
    --publish 80:80 \
    --volume /etc/letsencrypt:/etc/letsencrypt \
    --volume /var/lib/letsencrypt:/var/lib/letsencrypt \
    "$CERTBOT_IMAGE" "$@"
}

issue() {
  local name=$1 extra=${2:-} email=${3:-}
  local args=(certonly --standalone --non-interactive --agree-tos --preferred-challenges http
    --cert-name "$name" -d "$name")
  [[ -z $extra || $extra == "$name" ]] || args+=(-d "$extra")
  if [[ -n $email ]]; then
    args+=(--email "$email" --no-eff-email)
  else
    args+=(--register-unsafely-without-email)
  fi
  certbot "${args[@]}"
}

renew() {
  local name certificate before after
  name=$(sed -n 's/^KET_CERTBOT_NAME=//p' "$root/.env" | tail -n 1)
  [[ -n $name ]] || fail 'KET_CERTBOT_NAME is missing from .env'
  certificate="/etc/letsencrypt/live/$name/cert.pem"
  before=$(sha256sum "$certificate" 2>/dev/null || true)
  certbot renew --non-interactive
  after=$(sha256sum "$certificate" 2>/dev/null || true)
  if [[ $before != "$after" ]]; then
    "$root/packaging/server/refresh-certificates.sh" --restart
  else
    printf 'Ket certificate is current; nothing to refresh.\n'
  fi
}

[[ $EUID -eq 0 ]] || fail 'run as root'
case ${1:-} in
  issue)
    shift
    [[ $# -ge 1 ]] || fail 'issue needs a certificate name'
    issue "$@"
    ;;
  renew) renew ;;
  *) fail 'usage: certificates.sh issue NAME [EXTRA_NAME] [EMAIL] | renew' ;;
esac
