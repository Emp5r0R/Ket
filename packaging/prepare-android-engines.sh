#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
app_dir=${1:-"$repo_root/apps/ket-android/app"}
output="$app_dir/build/generated/ket-engines"
stamp="$output/versions"
hev_version=2.14.0
hev_checksum=f0c5909b188272a6cee2b3c92e13cf16d927ba29a20bd1d750a2ff3419cda381
openvpn_android_version=0.7.64
openvpn_android_checksum=50eaa5539778ce20fe3ed23e097aa811cce45be8eeea39904e31984c98c0b74e
versions="hysteria=v2.10.0
shadowsocks=1.24.0
xray=v26.3.27
wstunnel=10.6.2
openvpn-for-android=$openvpn_android_version
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
  "$output/jniLibs/arm64-v8a/libwstunnel.so" \
  "$output/hev-socks5-tunnel/Android.mk"; do
  [[ -f "$artifact" ]] || complete=false
done
for abi in armeabi-v7a arm64-v8a x86 x86_64; do
  for library in libopenvpn.so libovpnexec.so; do
    [[ -f "$output/jniLibs/$abi/$library" ]] || complete=false
  done
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
"$repo_root/packaging/fetch-wstunnel.sh" android-arm64 \
  "$stage/jniLibs/arm64-v8a/libwstunnel.so"

url="https://github.com/schwabe/ics-openvpn/releases/download/v${openvpn_android_version}/ics-openvpn-${openvpn_android_version}.apk"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
  --speed-limit 1024 --speed-time 30 \
  --output "$archive" "$url"
printf '%s  %s\n' "$openvpn_android_checksum" "$archive" | sha256sum --check --status
for abi in armeabi-v7a arm64-v8a x86 x86_64; do
  mkdir -p "$stage/jniLibs/$abi"
  for library in libopenvpn.so libovpnexec.so; do
    unzip -p "$archive" "lib/$abi/$library" > "$stage/jniLibs/$abi/$library"
    [[ -s "$stage/jniLibs/$abi/$library" ]]
    chmod 0755 "$stage/jniLibs/$abi/$library"
  done
done

url="https://github.com/heiher/hev-socks5-tunnel/releases/download/${hev_version}/hev-socks5-tunnel-${hev_version}.tar.xz"
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  --connect-timeout 15 --max-time 600 --retry 3 --retry-all-errors --retry-delay 2 \
  --speed-limit 1024 --speed-time 30 \
  --output "$archive" "$url"
printf '%s  %s\n' "$hev_checksum" "$archive" | sha256sum --check --status
tar -xJf "$archive" -C "$stage"
mv "$stage/hev-socks5-tunnel-${hev_version}" "$stage/hev-socks5-tunnel"
printf '%s\n' "$versions" > "$stage/versions"

rm -rf "$output"
mv "$stage" "$output"
trap - EXIT
rm -f "$archive"
printf 'Prepared Hysteria v2.10.0, shadowsocks-rust 1.24.0, Xray v26.3.27, wstunnel 10.6.2 (arm64), OpenVPN for Android %s, and hev-socks5-tunnel %s for Android.\n' \
  "$openvpn_android_version" "$hev_version"
