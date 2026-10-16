#!/usr/bin/env bash
# Builds Ket's invite verifier (crates/ket-invite-android) for the Android app.
#
#   build-android-invite.sh android <ndk-dir> <output-dir>
#       One libket_invite_jni.so per ABI under <output-dir>/jniLibs/<abi>/.
#   build-android-invite.sh host <output-dir>
#       A host build in <output-dir> for the app's JVM unit tests.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
library=libket_invite_jni.so
min_api=26
# ABI, Rust target, and NDK clang prefix.
abis=(
  "armeabi-v7a armv7-linux-androideabi armv7a-linux-androideabi"
  "arm64-v8a aarch64-linux-android aarch64-linux-android"
  "x86 i686-linux-android i686-linux-android"
  "x86_64 x86_64-linux-android x86_64-linux-android"
)

fail() {
  printf 'build-android-invite: %s\n' "$*" >&2
  exit 1
}

cargo_target_dir() {
  cargo metadata --format-version 1 --no-deps --manifest-path "$repo_root/Cargo.toml" |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p'
}

# Replaces <output> only once every file is in place.
publish() {
  local stage=$1 output=$2
  rm -rf "$output"
  mkdir -p "$(dirname "$output")"
  mv "$stage" "$output"
}

build_host() {
  local output=$1 target_dir stage
  cargo build --locked --manifest-path "$repo_root/Cargo.toml" -p ket-invite-android
  target_dir=$(cargo_target_dir)
  [[ -n "$target_dir" ]] || fail 'cannot find the Cargo target directory'
  stage=$(mktemp -d "${output%/}.XXXXXX")
  cp "$target_dir/debug/$library" "$stage/$library"
  publish "$stage" "$output"
}

build_android() {
  local ndk=$1 output=$2 toolchain target_dir stage entry abi target clang env_target
  toolchain="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
  [[ -x "$toolchain/llvm-ar" ]] || fail "no NDK LLVM toolchain under $ndk"
  target_dir=$(cargo_target_dir)
  [[ -n "$target_dir" ]] || fail 'cannot find the Cargo target directory'
  stage=$(mktemp -d "${output%/}.XXXXXX")
  trap 'rm -rf "$stage"' EXIT
  for entry in "${abis[@]}"; do
    read -r abi target clang <<<"$entry"
    rustup target list --installed 2>/dev/null | grep -qx "$target" ||
      fail "the Rust target $target is missing; run: rustup target add $target"
    env_target=$(tr 'a-z-' 'A-Z_' <<<"$target")
    env \
      "CARGO_TARGET_${env_target}_LINKER=$toolchain/${clang}${min_api}-clang" \
      "CARGO_TARGET_${env_target}_AR=$toolchain/llvm-ar" \
      "CARGO_TARGET_${env_target}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384" \
      cargo build --locked --release --manifest-path "$repo_root/Cargo.toml" \
      -p ket-invite-android --target "$target"
    mkdir -p "$stage/jniLibs/$abi"
    cp "$target_dir/$target/release/$library" "$stage/jniLibs/$abi/$library"
  done
  publish "$stage" "$output"
  trap - EXIT
}

case "${1:-}" in
  android)
    [[ $# -eq 3 ]] || fail 'usage: build-android-invite.sh android <ndk-dir> <output-dir>'
    build_android "$2" "$3"
    ;;
  host)
    [[ $# -eq 2 ]] || fail 'usage: build-android-invite.sh host <output-dir>'
    build_host "$2"
    ;;
  *)
    fail 'usage: build-android-invite.sh android <ndk-dir> <output-dir> | host <output-dir>'
    ;;
esac
