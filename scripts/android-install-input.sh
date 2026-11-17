#!/bin/sh
set -eu
. "$(dirname -- "$0")/android-env.sh"
if [ ! -e "$AI_PHONE_PROJECT_ROOT/runtime/services" ] && [ ! -L "$AI_PHONE_PROJECT_ROOT/runtime/services" ]; then
  cargo run --quiet --locked --manifest-path "$AI_PHONE_PROJECT_ROOT/Cargo.toml" -p ai-phone-release-tools -- phone-apps build "$AI_PHONE_PROJECT_ROOT"
  apk="$AI_PHONE_PROJECT_ROOT/.tools/phone-apps/input.apk"
else
  apk="$AI_PHONE_PROJECT_ROOT/runtime/phone-apps/input.apk"
fi
exec "$AI_PHONE_PROJECT_ROOT/scripts/android-maintenance" input "$apk"
