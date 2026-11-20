#!/bin/sh
set -eu

fixture=$(mktemp -d "$1/aiphone-workspace.XXXXXX")
bridge=$2
root="$fixture/rootfs"
source="$fixture/source"
work="$source/data/workspaces/admin"
bundle=/opt/aiphone/bundle
state=/var/lib/aiphone-linux
cleanup() {
  umount "$bundle" "$state"
  rm -rf -- "$fixture"
}
mkdir -p "$bundle" "$state" "$fixture/bundle" "$fixture/state"
mount --bind "$fixture/bundle" "$bundle"
mount --bind "$fixture/state" "$state"
trap cleanup EXIT

debootstrap --variant=minbase noble "$root" http://archive.ubuntu.com/ubuntu
tar -C "$root" -cf - . | zstd -q -o "$bundle/images.payload"
digest=$(sha256sum "$bundle/images.payload" | cut -d ' ' -f 1)
install -d -m 0755 "$source/runtime/services" "$source/data/runtime-skills"
chmod 0755 "$fixture"
install -d -o "$SUDO_UID" -g "$SUDO_GID" -m 0755 "$work" "$work/.home"
printf '{"linux_rootfs":{"sha256":"%s"}}\n' "$digest" > "$source/runtime/services/manifest.json"
printf 'retain\n' > "$source/data/runtime-skills/proof"
printf 'existing\n' > "$work/user-file"
"$bridge" native-linux "$source" stage

mkfifo "$fixture/lease"
exec 3<> "$fixture/lease"
"$bridge" native-linux "$source" workspace \
  'test "$(id -u)" != 0; grep -Fx retain /skills/proof; test ! -w /skills; printf "result\n" > output' '' \
  < "$fixture/lease" 3>&-
grep -Fx result "$work/output"

"$bridge" native-linux "$source" workspace \
  'printf "started\n" > running; sleep 12; printf "wrong\n" > late-output' '' \
  < "$fixture/lease" 3>&- &
worker=$!
for attempt in $(seq 1 100); do
  test -e "$work/running" && break
  sleep 0.05
done
exec 3>&-
status=0
wait "$worker" || status=$?
test "$status" -eq 130
test -f "$work/running"
test ! -e "$work/late-output"
test -z "$(cat /sys/fs/cgroup/aiphone-workspace/cgroup.procs)"
grep -Fx existing "$work/user-file"
