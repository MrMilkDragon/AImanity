#!/usr/bin/env bash
set -euo pipefail

tools=$(realpath -m "$1")
mkdir -p "$tools"/{bin,lib/qemu,share/qemu,licenses}
modules=/usr/lib/x86_64-linux-gnu/qemu
files=(/usr/bin/{qemu-system-x86_64,qemu-img,adb,espeak-ng} "$modules"/hw-display-virtio-gpu{,-pci}.so)
install -m755 -t "$tools/bin" "${files[@]:0:4}"
install -m644 -t "$tools/lib/qemu" "${files[@]:4}"
cp -a /usr/lib/x86_64-linux-gnu/espeak-ng-data "$tools/share/"
firmware=(/usr/share/seabios/bios-256k.bin /usr/share/qemu/{kvmvapic,linuxboot_dma}.bin)
install -m644 -t "$tools/share/qemu" "${firmware[@]}"

# Keep libc and the ELF loader paired with the user's operating system.
while read -r soname path; do
    case "$soname" in
        libc.so.*|libm.so.*|libdl.so.*|libpthread.so.*|librt.so.*|libresolv.so.*|libutil.so.*) continue ;;
    esac
    install -m644 "$path" "$tools/lib/$soname"
    files+=("$path")
done < <(ldd "${files[@]}" | awk '$2 == "=>" && $3 ~ /^\// {print $1, $3}' | sort -u)

owners=$(mktemp)
trap 'rm -f "$owners"' EXIT
for path in "${files[@]}" "${firmware[@]}" /usr/lib/x86_64-linux-gnu/espeak-ng-data/intonations; do
    dpkg-query -S "$path" 2>/dev/null || dpkg-query -S "$(readlink -f "$path")" 2>/dev/null || dpkg-query -S "/usr${path#/usr}"
done | sed -E 's/: \/.*//' | sort -u > "$owners"
while read -r binary; do
    name=${binary%%:*}
    cp -L "/usr/share/doc/$name/copyright" "$tools/licenses/$name.copyright"
    dpkg-query -W -f='${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n' "$binary"
done < "$owners" > "$tools/licenses/debian-packages.tsv"
cp -L /usr/share/common-licenses/* "$tools/licenses/"

export LD_LIBRARY_PATH="$tools/lib" QEMU_MODULE_DIR="$tools/lib/qemu"
"$tools/bin/qemu-img" --version
"$tools/bin/adb" version
"$tools/bin/qemu-system-x86_64" -L "$tools/share/qemu" -accel help | grep -wx kvm
"$tools/bin/qemu-system-x86_64" -L "$tools/share/qemu" -device virtio-gpu-pci,help >/dev/null
"$tools/bin/qemu-system-x86_64" -L "$tools/share/qemu" -device virtio-sound-pci,help >/dev/null
