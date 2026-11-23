#!/usr/bin/env bash
# Assemble this tree inside a private mount namespace on an ARM64 Linux builder.
set -euo pipefail
compiled=$1
work=$2
out=$3/linux
keyring=$4
relay=$5
root="$work/root"
test "$(id -u)" = 0
test "$(uname -m)" = aarch64
mkdir -p "$work" "$out"
test ! -e "$root"
debootstrap --keyring="$keyring" --arch=arm64 --variant=minbase \
  trixie "$root" https://deb.debian.org/debian /usr/share/debootstrap/scripts/sid
printf '#!/bin/sh\nexit 101\n' > "$root/usr/sbin/policy-rc.d"
chmod 0755 "$root/usr/sbin/policy-rc.d"
mount --rbind /dev "$root/dev"
mount --make-rslave "$root/dev"
mount -t proc proc "$root/proc"
mount -t sysfs -o ro sysfs "$root/sys"
cleanup() {
  umount "$root/sys"
  umount "$root/proc"
  umount -R "$root/dev"
}
trap cleanup EXIT
# Filter the generated initramfs to the virtio platform actually used by both crosvm and QEMU.
mkdir -p "$root/etc/initramfs-tools/conf.d" "$root/etc/postgresql-common"
printf 'MODULES=list\nCOMPRESS=gzip\n' > "$root/etc/initramfs-tools/conf.d/avf"
printf 'create_main_cluster = false\n' > "$root/etc/postgresql-common/createcluster.conf"
chroot "$root" apt-get update -qq
chroot "$root" /usr/bin/env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  systemd-sysv systemd-resolved udev linux-image-arm64 initramfs-tools kmod e2fsprogs \
  busybox-static ca-certificates adb iproute2 bubblewrap slirp4netns zstd xz-utils util-linux \
  postgresql-17 postgresql-client-17 redis-server redis-tools tesseract-ocr tesseract-ocr-eng \
  tesseract-ocr-chi-sim libtesseract-dev libleptonica-dev tzdata
version=$(chroot "$root" sh -c 'ls -1 /lib/modules | sort -V | tail -n 1')
test -s "$root/boot/config-$version"
for driver in virtio_pci virtio_mmio virtio_blk virtio_console virtio_net vmw_vsock_virtio_transport ext4; do
  if chroot "$root" modinfo -k "$version" "$driver" >/dev/null 2>&1; then
    echo "$driver" >> "$root/etc/initramfs-tools/modules"
  fi
done
for feature in VIRTIO_BLK VIRTIO_NET VIRTIO_VSOCKETS EXT4_FS NET_NS USER_NS; do
  grep -Eq "^CONFIG_${feature}=(m|y)$" "$root/boot/config-$version"
done
chroot "$root" update-initramfs -u -k "$version"
kernel="$root/boot/vmlinuz-$version"
case "$(od -An -tx1 -N4 "$kernel" | tr -d ' \n')" in
  1f8b*) gzip -dc "$kernel" > "$out/Image" ;;
  28b52ffd) zstd -dc "$kernel" > "$out/Image" ;;
  04224d18) lz4 -dc "$kernel" > "$out/Image" ;;
  *) cp "$kernel" "$out/Image" ;;
esac
test "$(od -An -tx1 -j56 -N4 "$out/Image" | tr -d ' \n')" = 41524d64
install -m 0644 "$root/boot/initrd.img-$version" "$out/initrd.img"

prefix="$root/opt/aiphone"
mkdir -p "$prefix"/{bundle,engine/bin,linux/bin,native-phone-bridge/platform-tools,postgres/lib,postgres/share,redis/bin} \
  "$root"/{var/lib/aiphone,usr/local/libexec,etc/systemd/network}
install -m 0755 "$compiled/bootstrap.program" "$root/usr/local/libexec/aiphone-bootstrap"
install -m 0755 "$relay" "$root/usr/local/libexec/aimanity-avf-guest-relay"
install -m 0644 "$compiled"/{source.payload,source.sha256,images.payload} "$prefix/bundle/"
(cd "$prefix/bundle" && echo "$(cat source.sha256)  source.payload" | sha256sum -c -)
ln -s /usr/lib/postgresql "$prefix/postgres/lib/postgresql"
ln -s /usr/share/postgresql "$prefix/postgres/share/postgresql"
ln -s /usr/share/zoneinfo "$prefix/linux/zoneinfo"
ln -s /usr/bin/busybox "$prefix/engine/bin/busybox"
ln -s /usr/bin/adb "$prefix/native-phone-bridge/platform-tools/adb"
# The unprivileged Redis service may create its own files. Keep the root-owned bin directory protected.
chown root:65534 "$prefix/redis"
chmod 1770 "$prefix/redis"
for app in redis-server redis-cli; do ln -s "/usr/bin/$app" "$prefix/redis/bin/$app"; done
for app in bwrap slirp4netns zstd; do ln -s "/usr/bin/$app" "$prefix/linux/bin/$app"; done
printf '/dev/vda / ext4 defaults 0 1\n' > "$root/etc/fstab"
printf 'aimanity\n' > "$root/etc/hostname"
truncate -s 0 "$root/etc/machine-id"
cat > "$root/etc/systemd/network/20-avf.network" <<'NETWORK'
[Match]
Type=ether
[Network]
DHCP=yes
IPv6AcceptRA=yes
NETWORK
ln -snf /run/systemd/resolve/stub-resolv.conf "$root/etc/resolv.conf"
install_units="$root/etc/systemd/system"
cat > "$install_units/aimanity-avf-relay.service" <<'UNIT'
[Unit]
Description=AImanity Android AVF transport
After=systemd-modules-load.service local-fs.target

[Service]
ExecStart=/usr/local/libexec/aimanity-avf-guest-relay
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
UNIT
cat > "$install_units/aimanity-avf-backend.service" <<'UNIT'
[Unit]
Description=AImanity workbench guest services
After=local-fs.target aimanity-avf-relay.service network.target
Requires=aimanity-avf-relay.service

[Service]
ExecStart=/usr/local/libexec/aiphone-bootstrap --avf-guest-control
Restart=on-failure
RestartSec=5
TimeoutStartSec=infinity
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
UNIT
chroot "$root" systemctl disable redis-server.service postgresql.service
chroot "$root" systemctl enable aimanity-avf-relay aimanity-avf-backend systemd-networkd systemd-resolved
for command in /opt/aiphone/redis/bin/redis-cli /opt/aiphone/postgres/lib/postgresql/17/bin/postgres \
  /opt/aiphone/linux/bin/bwrap /opt/aiphone/linux/bin/zstd /usr/local/libexec/aiphone-bootstrap; do
  chroot "$root" "$command" --version
done
chroot "$root" /opt/aiphone/native-phone-bridge/platform-tools/adb version
chroot "$root" /opt/aiphone/engine/bin/busybox --list | grep -x 'poweroff'
chroot "$root" apt-get clean
rm -f "$root/usr/sbin/policy-rc.d"
rm -rf "$root/var/lib/apt/lists"
# External direct-kernel boot reads these bytes from APK resources.
rm -f "$root/boot/vmlinuz-$version" "$root/boot/initrd.img-$version"
cleanup
trap - EXIT

seed="$work/rootfs.img"
usage=$(du -sx --block-size=1M "$root" | cut -f1)
blocks_mib=$((usage * 12 / 10 + 256))
truncate -s "${blocks_mib}M" "$seed"
mke2fs -q -F -t ext4 -m 0 -L aimanity-linux -d "$root" "$seed"
e2fsck -fn "$seed"
xz -3 -T2 -c "$seed" > "$out/rootfs.img.xz"
xz -3 -T2 "$out/Image"
cat > "$out/avf.json" <<'CONFIG'
{
  "name": "AImanity Linux",
  "kernel": "Image",
  "initrd": "initrd.img",
  "params": "console=hvc0 root=/dev/vda rootwait rw init=/sbin/init",
  "disks": [{"image":"rootfs.img","writable":true}],
  "protected": false,
  "platform_version": "~1.0"
}
CONFIG
