#!/usr/bin/env bash
set -euo pipefail

# Builds the Ket Android APKs and validates the shipped splits.
#
#   build-android.sh [debug|release] [--out DIR]
#
# --out copies the arm64-v8a and universal APKs into DIR under their release
# names (Ket_<version>_android-arm64.apk, Ket_<version>_android-universal.apk).
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
project_dir="$repo_root/apps/ket-android"
variant=debug
out_dir=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    debug | release) variant=$1; shift ;;
    --out)
      [[ $# -ge 2 ]] || { printf -- '--out requires a directory.\n' >&2; exit 2; }
      out_dir=$2
      shift 2
      ;;
    *)
      printf 'Usage: %s [debug|release] [--out DIR]\n' "$0" >&2
      exit 2
      ;;
  esac
done
apk_dir="$project_dir/app/build/outputs/apk/$variant"
# Published splits: release name suffix -> Gradle split name.
declare -A shipped_splits=([arm64]=arm64-v8a [universal]=universal)
case "$variant" in
  debug)
    gradle_task=:app:assembleDebug
    validation_args=()
    ;;
  release)
    gradle_task=:app:assembleRelease
    required=(
      KET_ANDROID_KEYSTORE
      KET_ANDROID_KEYSTORE_PASSWORD
      KET_ANDROID_KEY_ALIAS
      KET_ANDROID_KEY_PASSWORD
      KET_ANDROID_CERT_SHA256
    )
    for variable in "${required[@]}"; do
      if [[ -z "${!variable:-}" ]]; then
        printf '%s is required for a release build.\n' "$variable" >&2
        exit 1
      fi
    done
    if [[ ! -f "$KET_ANDROID_KEYSTORE" ]]; then
      printf 'KET_ANDROID_KEYSTORE does not name a file: %s\n' "$KET_ANDROID_KEYSTORE" >&2
      exit 1
    fi
    validation_args=(--expected-cert-sha256 "$KET_ANDROID_CERT_SHA256")
    ;;
esac

# Gradle derives versionName and versionCode from the workspace version. A
# release must not carry a stale override (for example from an old env file).
workspace_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo_root/Cargo.toml" | head -n 1)
if [[ $variant == release ]]; then
  IFS=. read -r major minor patch <<<"$workspace_version"
  workspace_code=$((10#$major * 10000 + 10#$minor * 100 + 10#$patch))
  if [[ -n ${KET_ANDROID_VERSION_NAME:-} && $KET_ANDROID_VERSION_NAME != "$workspace_version" ]] \
    || [[ -n ${KET_ANDROID_VERSION_CODE:-} && $KET_ANDROID_VERSION_CODE != "$workspace_code" ]]; then
    printf 'KET_ANDROID_VERSION_NAME/CODE (%s/%s) disagree with the workspace version %s (%s); unset them.\n' \
      "${KET_ANDROID_VERSION_NAME:-}" "${KET_ANDROID_VERSION_CODE:-}" "$workspace_version" "$workspace_code" >&2
    exit 1
  fi
fi

sdk=${KET_ANDROID_SDK:-${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}}
if [[ ! -d "$sdk/platforms" && -d "$repo_root/../Abyssal/android-sdk/platforms" ]]; then
  sdk="$repo_root/../Abyssal/android-sdk"
fi
if [[ -z "$sdk" || ! -d "$sdk/platforms" ]]; then
  printf 'Android SDK not found. Set KET_ANDROID_SDK or ANDROID_SDK_ROOT.\n' >&2
  exit 1
fi

printf 'sdk.dir=%s\n' "$sdk" > "$project_dir/local.properties"
apksigner="$sdk/build-tools/36.0.0/apksigner"
if [[ ! -x "$apksigner" ]]; then
  printf 'Android Build Tools 36.0.0 apksigner was not found under %s.\n' "$sdk" >&2
  exit 1
fi
"$repo_root/packaging/prepare-android-engines.sh" "$project_dir/app"
gradle_home=${GRADLE_USER_HOME:-/media/n_emperor/Aadhish/gradle-home}
(cd "$project_dir" && GRADLE_USER_HOME="$gradle_home" ./gradlew --no-daemon "$gradle_task")
version=${KET_ANDROID_VERSION_NAME:-$workspace_version}
[[ -z $out_dir ]] || mkdir -p "$out_dir"
for name in "${!shipped_splits[@]}"; do
  split=${shipped_splits[$name]}
  apk="$apk_dir/app-$split-$variant.apk"
  APKSIGNER="$apksigner" \
    "$repo_root/packaging/validate-android-apk.sh" --abi "$split" "${validation_args[@]}" "$apk"
  if [[ -n $out_dir ]]; then
    apk_out="$out_dir/Ket_${version}_android-$name.apk"
    [[ $variant == release ]] || apk_out="${apk_out%.apk}-debug.apk"
    cp -- "$apk" "$apk_out"
    apk=$apk_out
  fi
  printf 'APK: %s\n' "$apk"
  sha256sum "$apk"
done
