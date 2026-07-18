#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
project_dir="$repo_root/apps/ket-android"
sdk=${KET_ANDROID_SDK:-${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}}
if [[ ! -d "$sdk/platforms" && -d "$repo_root/../Abyssal/android-sdk/platforms" ]]; then
  sdk="$repo_root/../Abyssal/android-sdk"
fi
if [[ -z "$sdk" || ! -d "$sdk/platforms" ]]; then
  printf 'Android SDK not found. Set KET_ANDROID_SDK or ANDROID_SDK_ROOT.\n' >&2
  exit 1
fi

printf 'sdk.dir=%s\n' "$sdk" > "$project_dir/local.properties"
gradle_home=${GRADLE_USER_HOME:-/media/n_emperor/Aadhish/gradle-home}
(cd "$project_dir" && GRADLE_USER_HOME="$gradle_home" ./gradlew --no-daemon :app:assembleDebug)
apk="$project_dir/app/build/outputs/apk/debug/app-debug.apk"
printf 'APK: %s\n' "$apk"
sha256sum "$apk"
