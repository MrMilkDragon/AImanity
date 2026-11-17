#!/bin/sh
set -eu
. "$(dirname -- "$0")/android-env.sh"
if [ "${1:-}" = --observe-only ]; then
  exec "$AI_PHONE_PROJECT_ROOT/scripts/android-maintenance" ready --observe-only
fi
"$AI_PHONE_PROJECT_ROOT/scripts/android-maintenance" ready
adb="$AI_PHONE_PROJECT_ROOT/scripts/android-adb"
out="$AI_PHONE_PROJECT_ROOT/data/android/network-helper"
abi=$("$adb" shell getprop ro.product.cpu.abi | tr -d '\r')
cargo run --quiet --locked --manifest-path "$AI_PHONE_PROJECT_ROOT/Cargo.toml" -p ai-phone-release-tools -- \
  build-android-network "$AI_PHONE_PROJECT_ROOT" "$abi" "$out"
guest="/data/local/tmp/aimanity-network"
"$adb" shell mkdir -p "$guest"
"$adb" push "$out/network.dex" "$out/libaimanity_network.so" "$guest/" >/dev/null
exec "$adb" shell 'CLASSPATH=/data/local/tmp/aimanity-network/network.dex app_process / dev.aiphone.network.DataUsageWarnings --disable-warnings'
