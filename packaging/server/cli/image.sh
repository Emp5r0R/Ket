# shellcheck shell=bash
# The Ket server image for an installation tree. Sourced by the ket command
# and the installer; never executed.
# shellcheck disable=SC2034,SC2154

readonly KET_PUBLISHED_IMAGE=ghcr.io/emp5r0r/ket-control-plane

# prepare_image DIR REF TAG: makes ket-control-plane:TAG for the tree in DIR.
# A release tree uses the image published with that release when it exists
# and carries the release's version label; otherwise, or with
# KET_BUILD_IMAGE=1, the image is built from DIR.
prepare_image() {
  local dir=$1 ref=$2 tag=$3 version published label
  version=$(ref_version "$ref" || true)
  if [[ -n $version && -z ${KET_BUILD_IMAGE:-} ]]; then
    published="$KET_PUBLISHED_IMAGE:$version"
    if docker_cli pull --quiet "$published" >/dev/null 2>&1; then
      label=$(docker_cli image inspect --format '{{ index .Config.Labels "org.opencontainers.image.version" }}' "$published" 2>/dev/null || true)
      if [[ $label == "$version" ]]; then
        printf 'Using the published Ket %s server image.\n' "$version"
        docker_cli tag "$published" "$KET_IMAGE:$tag"
        return 0
      fi
      printf 'The published image does not match Ket %s; building it here instead.\n' "$version"
    fi
  fi
  printf 'Building the Ket server image (this takes a few minutes)...\n'
  docker_cli build --tag "$KET_IMAGE:$tag" "$dir"
}

image_exists() { docker_cli image inspect "$KET_IMAGE:$1" >/dev/null 2>&1; }
