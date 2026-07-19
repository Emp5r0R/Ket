#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
project_dir="$repo_root/apps/ket-android"
variant=${1:-debug}
case "$variant" in
  debug)
    gradle_task=:app:assembleDebug
    apk="$project_dir/app/build/outputs/apk/debug/app-debug.apk"
    validation_args=()
    ;;
  release)
    gradle_task=:app:assembleRelease
    apk="$project_dir/app/build/outputs/apk/release/app-release.apk"
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
  *)
    printf 'Usage: %s [debug|release]\n' "$0" >&2
    exit 2
    ;;
esac

sdk=${KET_ANDROID_SDK:-${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}}
if [[ ! -d "$sdk/platforms" && -d "$repo_root/../Abyssal/android-sdk/platforms" ]]; then
  sdk="$repo_root/../Abyssal/android-sdk"
fi
if [[ -z "$sdk" || ! -d "$sdk/platforms" ]]; then
  printf 'Android SDK not found. Set KET_ANDROID_SDK or ANDROID_SDK_ROOT.\n' >&2
  exit 1
fi

printf 'sdk.dir=%s\n' "$sdk" > "$project_dir/local.properties"
apksigner="$sdk/build-tools/34.0.0/apksigner"
if [[ ! -x "$apksigner" ]]; then
  printf 'Android Build Tools 34.0.0 apksigner was not found under %s.\n' "$sdk" >&2
  exit 1
fi
"$repo_root/packaging/prepare-android-engines.sh" "$project_dir/app"
gradle_home=${GRADLE_USER_HOME:-/media/n_emperor/Aadhish/gradle-home}
(cd "$project_dir" && GRADLE_USER_HOME="$gradle_home" ./gradlew --no-daemon "$gradle_task")
APKSIGNER="$apksigner" \
  "$repo_root/packaging/validate-android-apk.sh" "${validation_args[@]}" "$apk"
printf 'APK: %s\n' "$apk"
sha256sum "$apk"
