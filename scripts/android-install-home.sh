#!/bin/sh
set -eu
. "$(dirname -- "$0")/android-env.sh"
apk="$AI_PHONE_PROJECT_ROOT/runtime/phone-apps/home.apk"
if [ ! -e "$apk" ]; then
  cargo run --quiet --locked --manifest-path "$AI_PHONE_PROJECT_ROOT/Cargo.toml" -p ai-phone-release-tools -- build-android-apk "$AI_PHONE_PROJECT_ROOT" home >&2
  apk="$AI_PHONE_PROJECT_ROOT/data/android/home/ai-phone-home.apk"
fi
adb -P 15037 -s "${AI_PHONE_SERIAL:-emulator-5564}" install -r "$apk"
adb -P 15037 -s "${AI_PHONE_SERIAL:-emulator-5564}" shell cmd package set-home-activity dev.aiphone.home/.HomeActivity
adb -P 15037 -s "${AI_PHONE_SERIAL:-emulator-5564}" shell cmd input keyevent KEYCODE_HOME
