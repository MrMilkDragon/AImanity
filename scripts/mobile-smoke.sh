#!/usr/bin/env bash
set -euo pipefail
activity="$PACKAGE/dev.aimanity.mobile.WorkbenchActivity"
mkdir -p evidence
trap 'adb logcat -d -t 2500 > evidence/logcat.txt || true; adb shell dumpsys activity activities > evidence/activities.txt || true; adb exec-out screencap -p > evidence/last.png || true' EXIT

printf '%s  %s\n' "$APK_SHA256" "$APK" | sha256sum --check --strict
adb shell input keyevent KEYCODE_WAKEUP
adb shell wm dismiss-keyguard
adb shell settings put secure immersive_mode_confirmations confirmed
adb shell settings put secure show_ime_with_hard_keyboard 0
adb install "$APK"
apk_path=$(adb shell pm path "$PACKAGE" | tr -d '\r' | sed -n 's/^package://p')
installed_sha=$(adb shell sha256sum "$apk_path" | awk '{print $1}')
[[ $installed_sha == "$APK_SHA256" ]]
adb shell pm clear "$PACKAGE" | grep -F Success
adb shell am start -W -S -n "$activity" | tee evidence/cold-launch.txt
grep -F 'Status: ok' evidence/cold-launch.txt

# Locate rendered words in the actual framebuffer. All tap coordinates use
# this screenshot's pixel coordinates on the selected virtual hardware.
capture() {
  adb exec-out screencap -p > "evidence/$1.png"
  tesseract "evidence/$1.png" stdout -l eng --psm 11 tsv > "evidence/$1.tsv" 2>/dev/null
}
word_box() {
  awk -F '\t' -v word="$2" '
    $1 == 5 && index(tolower($12),tolower(word)) && (!found || $8 < y) {
      x=$7; y=$8; w=$9; h=$10; found=1
    }
    END { if (!found) exit 1; print x,y,w,h }
  ' "evidence/$1.tsv"
}
wait_word() {
  for attempt in {1..20}; do
    capture "$1"
    if word_box "$1" "$2" >/dev/null; then return 0; fi
    sleep 1
  done
  echo "Timed out looking for '$2' in $1" >&2
  return 1
}
tap_word() {
  local x y w h
  read -r x y w h < <(word_box "$1" "$2")
  adb shell input tap "$((x+w/2))" "$((y+h/2))"
}

wait_word languages English
read -r width height < <(identify -format '%w %h\n' evidence/languages.png)
(( width > height ))
adb shell am get-config > evidence/config.txt
smallest=$(sed -nE '/^config:/s/.*-sw([0-9]+)dp-.*/\1/p' evidence/config.txt | head -1)
[[ $smallest =~ ^[0-9]+$ ]]
case "$PROFILE" in
  phone) (( smallest < 600 ));;
  tablet) (( smallest >= 600 ));;
  *) exit 1;;
esac
tap_word languages English
wait_word selected Continue
tap_word selected Continue
wait_word settings devices

# Read and edit the first CPU value, next to the Phone label. The two
# adjacent field labels anchor the crop even if navigation insets move.
read -r cpu_x cpu_y cpu_w cpu_h < <(word_box settings CPU)
read -r mem_x mem_y mem_w mem_h < <(word_box settings Memory)
(( mem_x > cpu_x && cpu_h > 0 ))
read -r cx cy cw ch < <(awk -v x="$mem_x" -v y="$cpu_y" -v h="$cpu_h" -v height="$height" \
  'BEGIN {s=height/600; print int(x-41*s),int(y-3*s),int(20*s),int(h+8*s)}')
cpu_value() {
  local shot=$1
  convert "evidence/$shot.png" -crop "${cw}x${ch}+${cx}+${cy}" +repage -resize 400% \
    -colorspace Gray -contrast-stretch '2%x10%' -bordercolor white -border 12 "evidence/$shot-cpu.png"
  tesseract "evidence/$shot-cpu.png" stdout -l eng --psm 10 -c tessedit_char_whitelist=0123456789 2>/dev/null | tr -cd '0-9'
}
before=$(cpu_value settings)
[[ $before == 2 ]] || { echo "Expected initial phone CPU value 2, read '$before'" >&2; exit 1; }
adb shell input tap "$((cx+cw*3/4))" "$((cy+ch/2))"
adb shell input keyevent KEYCODE_MOVE_END KEYCODE_DEL
adb shell input text 4
adb shell input keyevent KEYCODE_ENTER
for attempt in {1..10}; do
  capture edited
  after=$(cpu_value edited)
  if [[ $after == 4 ]]; then break; fi
  sleep 1
done
[[ $after == 4 ]] || { echo "Expected edited phone CPU value 4, read '$after'" >&2; exit 1; }
adb shell dumpsys window > evidence/window.txt
grep -F 'mCurrentFocus=' evidence/window.txt | grep -F "$activity"
adb shell pidof "$PACKAGE"
jq -n --arg device "$AVD_DEVICE" --arg profile "$PROFILE" --arg sha256 "$installed_sha" \
  --argjson width "$width" --argjson height "$height" --argjson smallestWidthDp "$smallest" \
  --argjson before "$before" --argjson after "$after" \
  '{device:$device,profile:$profile,installedApkSha256:$sha256,$width,$height,$smallestWidthDp,
     input:{method:"adb-injected-key-events",screen:"English setup",field:"Phone CPU",before:$before,typed:"4",observed:$after}}' > evidence/receipt.json
cat evidence/receipt.json >> "$GITHUB_STEP_SUMMARY"
