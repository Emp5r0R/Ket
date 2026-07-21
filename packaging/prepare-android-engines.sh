#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
app_dir=${1:-"$repo_root/apps/ket-android/app"}
output="$app_dir/build/generated/ket-engines"
stamp="$output/versions"
hev_version=2.14.0
hev_checksum=f0c5909b188272a6cee2b3c92e13cf16d927ba29a20bd1d750a2ff3419cda381
versions="hysteria=v2.10.0
shadowsocks=1.24.0
xray=v26.3.27
hev-socks5-tunnel=$hev_version"

complete=true
for artifact in \
  "$output/jniLibs/armeabi-v7a/libhysteria.so" \
  "$output/jniLibs/arm64-v8a/libhysteria.so" \
  "$output/jniLibs/x86/libhysteria.so" \
  "$output/jniLibs/x86_64/libhysteria.so" \
  "$output/jniLibs/arm64-v8a/libsslocal.so" \
  "$output/jniLibs/x86_64/libsslocal.so" \
  "$output/jniLibs/arm64-v8a/libxray.so" \
  "$output/jniLibs/x86_64/libxray.so" \
  "$output/hev-socks5-tunnel/Android.mk"; do
  [[ -f "$artifact" ]] || complete=false
done
if [[ "$complete" == true && -f "$stamp" && "$(cat "$stamp")" == "$versions" ]]; then
  printf 'Android transport engines are already prepared.\n'
  exit 0
fi

mkdir -p "$app_dir/build" "$(dirname "$output")"
stage=$(mktemp -d "$app_dir/build/ket-engines.XXXXXX")
archive=$(mktemp)
cleanup() {
  rm -rf "$stage"
  rm -f "$archive"
}
trap cleanup EXIT

"$repo_root/packaging/fetch-hysteria.sh" android-armv7 \
  "$stage/jniLibs/armeabi-v7a/libhysteria.so"
"$repo_root/packaging/fetch-hysteria.sh" android-arm64 \
  "$stage/jniLibs/arm64-v8a/libhysteria.so"
"$repo_root/packaging/fetch-hysteria.sh" android-386 \
  "$stage/jniLibs/x86/libhysteria.so"
"$repo_root/packaging/fetch-hysteria.sh" android-amd64 \
  "$stage/jniLibs/x86_64/libhysteria.so"
"$repo_root/packaging/fetch-shadowsocks.sh" android-arm64 \
  "$stage/jniLibs/arm64-v8a/libsslocal.so"
"$repo_root/packaging/fetch-shadowsocks.sh" android-amd64 \
  "$stage/jniLibs/x86_64/libsslocal.so"
"$repo_root/packaging/fetch-xray.sh" android-arm64 \
  "$stage/jniLibs/arm64-v8a/libxray.so"
"$repo_root/packaging/fetch-xray.sh" android-amd64 \
  "$stage/jniLibs/x86_64/libxray.so"

url="https://github.com/heiher/hev-socks5-tunnel/releases/download/${hev_version}/hev-socks5-tunnel-${hev_version}.tar.xz"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --output "$archive" "$url"
printf '%s  %s\n' "$hev_checksum" "$archive" | sha256sum --check --status
tar -xJf "$archive" -C "$stage"
mv "$stage/hev-socks5-tunnel-${hev_version}" "$stage/hev-socks5-tunnel"
printf '%s\n' "$versions" > "$stage/versions"

rm -rf "$output"
mv "$stage" "$output"
trap - EXIT
rm -f "$archive"
printf 'Prepared Hysteria v2.10.0, shadowsocks-rust 1.24.0, Xray v26.3.27, and hev-socks5-tunnel %s for Android.\n' "$hev_version"
