#!/usr/bin/env bash
set -euo pipefail
src=$(realpath "${1:?Android ARM64 input directory}")
out=$(realpath -m "${2:?New asset directory}")
mkdir -p "$out/android" "$out/speech"
test ! -e "$out/android/avf.json"
guest="$out/android"
images="$src/android"
cargo run --quiet --locked -p ai-phone-release-tools -- android-avf-boot "$images" "$guest"

# Decode the embedded kernel and validate the Linux arm64 Image header.
magic() { od -An -v -tx1 -j "${2:-0}" -N 4 "$1" | tr -d ' \n'; }
kernel="$guest/kernel.payload"
case "$(magic "$kernel")" in
  1f8b*) gzip -dc "$kernel" > "$guest/Image" ;;
  02214c18|04224d18) lz4 -dc "$kernel" > "$guest/Image" ;;
  *) mv "$kernel" "$guest/Image" ;;
esac
[[ "$(magic "$guest/Image" 56)" == 41524d64 ]]
[[ "$(tail -c 12 "$guest/initrd.img")" == '#BOOTCONFIG' ]]
params=$(cat "$guest/vendor-cmdline")
rm -f "$kernel" "$guest/vendor-cmdline"

for name in boot init_boot vendor_boot vbmeta vbmeta_system vbmeta_system_dlkm vbmeta_vendor_dlkm super userdata; do
  img="$images/$name.img"
  test -s "$img"
  if [[ "$(magic "$img")" == 3aff26ed ]]; then
    simg2img "$img" "$guest/$name.img"
  else
    cp --sparse=always "$img" "$guest/$name.img"
  fi
done
truncate -s 1M "$guest/misc.img" "$guest/frp.img"
truncate -s 64M "$guest/metadata.img"

# AVF assembles the GPT from these labels. The guest-private writable
# partitions are seeded once, so Android keeps its data across restarts.
jq -n --arg params "$params bootconfig console=hvc0" '{
  name:"AImanity Android", kernel:"Image", initrd:"initrd.img", $params,
  console_input_device:"hvc0", protected:false, memory_mib:4096, cpu_topology:"match_host", platform_version:"~1.0",
  disks:[
    {writable:false, partitions:(
      [("boot","init_boot","vendor_boot","vbmeta","vbmeta_system","vbmeta_system_dlkm","vbmeta_vendor_dlkm") as $p |
        ("a","b") as $slot | {label:($p+"_"+$slot),path:($p+".img"),writable:false}] +
      [{label:"super",path:"super.img",writable:false}])},
    {writable:true, partitions:[("misc","frp","metadata","userdata") as $p |
      {label:$p,path:($p+".img"),writable:true}]}
  ]}' > "$guest/avf.json"
(
  cd "$guest"
  jq -r '.kernel,.initrd,.disks[].partitions[].path' avf.json | sort -u |
    while IFS= read -r name; do
      test -s "$name"
      printf '%s %s bytes\n' "$name" "$(stat -c %s "$name")"
      xz -3 -T2 "$name"
    done
)
cargo run --quiet --locked -p ai-phone-release-tools -- mobile-guest-apps "$PWD" "$guest/apps"
cp "$src/speech/ggml-base-q5_1.bin" "$out/speech/"
