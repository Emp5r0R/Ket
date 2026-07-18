#!/usr/bin/env bash
set -euo pipefail

image=${1:-}
if [[ -z "$image" ]]; then
  printf 'Usage: %s <registry/image:tag>\n' "$0" >&2
  exit 2
fi

if ! docker buildx version >/dev/null 2>&1; then
  printf 'Docker Buildx is required for multi-architecture publishing.\n' >&2
  exit 1
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --tag "$image" \
  --file "$repo_root/Dockerfile" \
  --push \
  "$repo_root"
