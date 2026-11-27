#!/usr/bin/env bash
set -euo pipefail
image=$1
mount="$RUNNER_TEMP/overlay-mount"
install="$RUNNER_TEMP/overlay-installed/AImanity.app"
mkdir -p "$mount" "$RUNNER_TEMP/overlay-installed"
codesign --verify --strict "$image"
hdiutil attach -readonly -nobrowse -mountpoint "$mount" "$image"
if [[ -d $install ]]; then rm -rf "$install"; fi
ditto "$mount/AImanity.app" "$install"
hdiutil detach "$mount"
codesign --verify --deep --strict "$install"
binary=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$install/Contents/Info.plist")
printf 'AIMANITY_QA_CLIENT=%s\n' "$install/Contents/MacOS/$binary" >> "$GITHUB_ENV"
