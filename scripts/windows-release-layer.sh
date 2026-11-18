#!/usr/bin/env bash
set -euo pipefail

compiled=$(realpath "$1")
baseline=$(realpath "$2")
out=$(realpath "$3")
release=$4
temp=$(mktemp -d "$out/.release-layer.XXXXXXXX")
trap 'rm -rf "$temp"' EXIT

base="$baseline/host/qemu/guest"
grep -qx 'CONFIG_BLK_DEV_INITRD=y' "$base/kernel.config"
grep -qx 'CONFIG_RD_GZIP=y' "$base/kernel.config"
mkdir -p "$temp/init"/{bin,dev,lib,newroot} "$temp/layer"
qemu-img convert -f qcow2 -O raw -S 4096 "$base/guest.qcow2" "$temp/base.ext4"
debugfs -R "dump /opt/aiphone/engine/bin/busybox $temp/init/bin/busybox" "$temp/base.ext4"
debugfs -R "dump /opt/aiphone/engine/lib/ld-musl-x86_64.so.1 $temp/init/lib/ld-musl-x86_64.so.1" "$temp/base.ext4"
rm "$temp/base.ext4"
chmod 0755 "$temp/init/bin/busybox" "$temp/init/lib/ld-musl-x86_64.so.1"
sudo chroot "$temp/init" /bin/busybox --list > "$temp/applets"
for applet in mkdir mount sh sleep switch_root timeout; do
  grep -qx "$applet" "$temp/applets"
done
cat > "$temp/init/init" <<'INIT'
#!/bin/busybox sh
set -eu
/bin/busybox mount -t devtmpfs devtmpfs /dev
/bin/busybox timeout 20 /bin/busybox sh -c 'until [ -b /dev/vda ] && [ -b /dev/vdb ]; do /bin/busybox sleep 1; done'
/bin/busybox mount -t ext4 /dev/vda /newroot
/bin/busybox mount -t ext4 -o ro /dev/vdb /newroot/mnt
/bin/busybox mount -o bind /newroot/mnt/bootstrap.program /newroot/usr/local/libexec/aiphone-bootstrap
/bin/busybox mount -o bind /newroot/mnt/source.payload /newroot/opt/aiphone/bundle/source.payload
/bin/busybox mount -o bind /newroot/mnt/source.sha256 /newroot/opt/aiphone/bundle/source.sha256
/bin/busybox mount -o bind /newroot/mnt/images.payload /newroot/opt/aiphone/bundle/images.payload
/bin/busybox mkdir -p /newroot/opt/aiphone/redis
/bin/busybox mount -o bind /newroot/mnt/redis /newroot/opt/aiphone/redis
/bin/busybox mkdir -p /newroot/opt/aiphone/postgres
/bin/busybox mount -o bind /newroot/mnt/postgres /newroot/opt/aiphone/postgres
/bin/busybox mkdir -p /newroot/opt/aiphone/linux /newroot/usr/share/zoneinfo
/bin/busybox mount -o bind /newroot/mnt/linux /newroot/opt/aiphone/linux
/bin/busybox mount -o bind /newroot/mnt/linux/zoneinfo /newroot/usr/share/zoneinfo
/bin/busybox mount -o move /dev /newroot/dev
read -r release < /newroot/opt/aiphone/bundle/source.sha256
printf 'AI_PHONE_RELEASE_LAYER %s\n' "$release"
exec /bin/busybox switch_root /newroot /sbin/init
INIT
chmod 0755 "$temp/init/init"
(
  cd "$temp/init"
  find . -print0 | LC_ALL=C sort -z | cpio --null --quiet -o -H newc --owner=0:0 | gzip -n > "$temp/release-init.cpio.gz"
)

install -m 0755 "$compiled/bootstrap.program" "$temp/layer/bootstrap.program"
install -m 0644 "$compiled/source.payload" "$temp/layer/source.payload"
printf '%s\n' "$release" > "$temp/layer/source.sha256"
install -m 0644 "$compiled/images.payload" "$temp/layer/images.payload"
cp -R "$compiled/redis" "$temp/layer/redis"
cp -R "$compiled/postgres" "$temp/layer/postgres"
cp -R "$compiled/linux" "$temp/layer/linux"
geometry=$(find "$temp/layer" -printf '%y %s\n' | awk '
  { inodes++ }
  $1 == "f" { blocks += int(($2 + 4095) / 4096) }
  END { print blocks + 4096, inodes + 32 }
')
read -r block_count inode_count <<< "$geometry"
mke2fs -q -F -t ext4 -b 4096 -m 0 -O ^has_journal -N "$inode_count" -d "$temp/layer" "$temp/release.ext4" "$block_count"
e2fsck -fn "$temp/release.ext4"
mv "$temp/release-init.cpio.gz" "$temp/release.ext4" "$out/"
