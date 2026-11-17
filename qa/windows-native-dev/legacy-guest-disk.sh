#!/usr/bin/env bash
set -euo pipefail
base=$1
out=$2
mkdir "$out"
work=$(mktemp -d "${RUNNER_TEMP:?}/calcium-qa.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
raw="$work/guest.ext4"
marker=/var/lib/aiphone/installation/user/data/desktop/home/Files/calcium-user-file.txt
printf 'Calcium 0.0.3 disk %s · 已保留\n' "${GITHUB_RUN_ID:?}" > "$out/marker.txt"
qemu-img convert -f qcow2 -O raw -S 4096 "$base" "$raw"
cat > "$work/seed.debugfs" <<EOF
mkdir /var/lib/aiphone
mkdir /var/lib/aiphone/installation
mkdir /var/lib/aiphone/installation/user
mkdir /var/lib/aiphone/installation/user/data
mkdir /var/lib/aiphone/installation/user/data/desktop
mkdir /var/lib/aiphone/installation/user/data/desktop/home
mkdir /var/lib/aiphone/installation/user/data/desktop/home/Files
write $out/marker.txt $marker
set_inode_field $marker uid 1000
set_inode_field $marker gid 1000
set_inode_field /var/lib/aiphone/installation/user/data/desktop/home uid 1000
set_inode_field /var/lib/aiphone/installation/user/data/desktop/home gid 1000
set_inode_field /var/lib/aiphone/installation/user/data/desktop/home/Files uid 1000
set_inode_field /var/lib/aiphone/installation/user/data/desktop/home/Files gid 1000
EOF
debugfs -w -f "$work/seed.debugfs" "$raw"
debugfs -R "dump $marker $work/readback" "$raw"
cmp "$out/marker.txt" "$work/readback"
e2fsck -fn "$raw"
qemu-img create -f qcow2 -F raw -b "$raw" "$out/user.qcow2"
qemu-img rebase -f qcow2 -F qcow2 -b "$base" "$out/user.qcow2"
qemu-img compare -f raw -F qcow2 "$raw" "$out/user.qcow2"
qemu-img resize -f qcow2 "$out/user.qcow2" 32G
qemu-img check -f qcow2 "$out/user.qcow2"
