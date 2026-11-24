#!/usr/bin/env bash
set -euo pipefail

fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
bin=$(cd "${CARGO_TARGET_DIR:-target}" && pwd)
resources=apps/native/gen/host
receipt=$(jq -er '."source.payload"' "$resources/integrity.json")
staging=$("$bin/debug/host-resources" stage "$resources/source.payload" "$fixture/staging" "$receipt")
staged=${staging#* }
"$bin/debug/release-tools" native-services verify "$staged"
