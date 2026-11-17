#!/bin/sh
set -eu
. "$(dirname -- "$0")/android-env.sh"
if [ "${1:-}" = --observe-only ]; then
  exec "$AI_PHONE_PROJECT_ROOT/scripts/android-maintenance" ready --observe-only
fi
"$AI_PHONE_PROJECT_ROOT/scripts/android-maintenance" ready
out="$AI_PHONE_PROJECT_ROOT/data/android/network-helper"
source="$AI_PHONE_PROJECT_ROOT/scripts/android-network/DataUsageWarnings.java"
helper="$out/data-usage-warnings.jar"
if [ ! -f "$helper" ] || [ "$source" -nt "$helper" ]; then
  mkdir -p "$out/classes" "$out/dex"
  platform="$ANDROID_HOME/platforms/android-35/android.jar"
  "$JAVA_HOME/bin/javac" --release 8 -classpath "$platform" -d "$out/classes" "$source"
  "$ANDROID_HOME/build-tools/35.0.0/d8" --lib "$platform" --min-api 26 --output "$out/dex" "$out/classes/dev/aiphone/network/DataUsageWarnings.class"
  "$JAVA_HOME/bin/jar" cf "$helper" -C "$out/dex" classes.dex
fi
adb="$AI_PHONE_PROJECT_ROOT/scripts/android-adb"
"$adb" push "$helper" /data/local/tmp/ai-phone-data-usage-warnings.jar >/dev/null
exec "$adb" shell 'CLASSPATH=/data/local/tmp/ai-phone-data-usage-warnings.jar app_process / dev.aiphone.network.DataUsageWarnings --disable-warnings'
