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
grep -F 'LaunchState: COLD' evidence/cold-launch.txt

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
{
  adb shell wm size
  adb shell wm density
  adb shell getprop ro.build.fingerprint
  adb shell getprop ro.product.cpu.abi
} > evidence/device.txt
smallest=$(sed -nE '/^config:/s/.*-sw([0-9]+)dp-.*/\1/p' evidence/config.txt | head -1)
[[ $smallest =~ ^[0-9]+$ ]]
case "$PROFILE" in
  phone) (( smallest < 600 ));;
  tablet) (( smallest >= 600 ));;
  *) exit 1;;
esac
for attempt in {1..8}; do
  # Activity creation can resize the surface after the first present.
  # Retap only while the actual localized heading has yet to change.
  capture languages
  if word_box languages English >/dev/null; then tap_word languages English; fi
  sleep 1
  capture selected
  if word_box selected Choose >/dev/null; then break; fi
done
word_box selected Choose >/dev/null
# On the language page, Continue is the centered solid button below the
# options. Its height and position follow the rendered canvas. Read the
# button from the framebuffer when OCR misses the reversed white text.
button_y=$(convert evidence/selected.png -colorspace Gray -threshold '25%' -depth 8 gray:- | \
  ruby -e '
    w,h = ARGV.map(&:to_i)
    image = STDIN.binmode.read
    abort "Unexpected grayscale frame size" unless image.bytesize == w*h
    bands = []
    first = nil
    (h/2..h*97/100).each do |y|
      row = image.byteslice(y*w + w*35/100, w*30/100)
      if row.count("\x00") > w*0.04 then first ||= y
      elsif first
        bands << [first,y-1]
        first = nil
      end
    end
    button = bands.select { |top,bottom| bottom-top > h*0.018 }.max_by { |top,bottom| bottom-top }
    abort "No centered action button in selected language frame" unless button
    puts button.sum/2
  ' "$width" "$height")
adb shell input tap "$((width/2))" "$button_y"
wait_word settings devices
sleep 1
capture settings

# Read and edit the first CPU value, next to the Phone label. The two
# adjacent field labels anchor the crop even if navigation insets move.
read -r cpu_x cpu_y cpu_w cpu_h < <(word_box settings CPU)
read -r mem_x mem_y mem_w mem_h < <(awk -F '\t' -v x="$cpu_x" -v y="$cpu_y" -v h="$cpu_h" '
  $1 == 5 && tolower($12) == "memory" && $7 > x && $8 <= y+h && $8+$10 >= y {
    print $7,$8,$9,$10; exit
  }' evidence/settings.tsv)
(( mem_x > cpu_x && cpu_h > 0 ))
value_box=$(awk -F '\t' -v left="$((cpu_x+cpu_w))" -v right="$mem_x" -v y="$cpu_y" -v h="$cpu_h" '
  $1 == 5 && $12 == "2" && $7 >= left && $7+$9 < right && $8 < y+h && $8+$10 > y {
    pad=int($10/3)+1; print $7-pad, $8-pad, $9+2*pad, $10+2*pad; exit
  }' evidence/settings.tsv)
if [[ -z $value_box ]]; then
  # The CPU and Memory label origins are 158 logical units apart in setup.
  value_box=$(awk -v x="$mem_x" -v l="$cpu_x" -v y="$cpu_y" -v h="$cpu_h" \
    'BEGIN {s=(x-l)/158; print int(x-41*s),int(y-3*s),int(20*s),int(h+8*s)}')
fi
read -r cx cy cw ch <<< "$value_box"
cpu_value() {
  local shot=$1 recognized
  convert "evidence/$shot.png" -crop "${cw}x${ch}+${cx}+${cy}" +repage -resize 400% \
    -colorspace Gray -contrast-stretch '2%x10%' -bordercolor white -border 12 "evidence/$shot-cpu.png"
  recognized=$(awk -F '\t' -v x="$cx" -v y="$cy" -v w="$cw" -v h="$ch" '
    $1 == 5 && $12 ~ /^[124]$/ && $7+$9/2 >= x && $7+$9/2 <= x+w && $8+$10/2 >= y && $8+$10/2 <= y+h {print $12; exit}
  ' "evidence/$shot.tsv")
  if [[ -n $recognized ]]; then printf %s "$recognized"; return; fi
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

# Try the installed system IME as well. Some AVD keyboard layouts don't
# expose digit keys to OCR. Record this separately from hardware key input.
tap_word settings Memory
adb shell settings put secure show_ime_with_hard_keyboard 1
adb shell input tap "$((cx+cw*3/4))" "$((cy+ch/2))"
sleep 1
adb shell dumpsys input_method > evidence/input-method.txt
ime_status=keyboard-not-detected
if grep -qE 'mInputShown=true|inputShown=true' evidence/input-method.txt; then
  capture keyboard
  key_location() {
    awk -F '\t' -v key="$2" -v h="$height" '
      $1 == 5 && $8 > h * 0.55 && $12 ~ key {print int($7+$9/2),int($8+$10/2); exit}
    ' "evidence/$1.tsv"
  }
  # Start with the symbols layout where a tap on the numeral commits it.
  # A numeral printed as a secondary hint on a letter needs a long press.
  symbols=$(key_location keyboard '123')
  ime_status=key-layout-unavailable
  if [[ -n $symbols ]]; then
    read -r sx sy <<< "$symbols"
    adb shell input keyevent KEYCODE_MOVE_END KEYCODE_DEL
    adb shell input tap "$sx" "$sy"
    sleep 0.5
    capture symbols
    digit=$(key_location symbols '^2$')
    if [[ -n $digit ]]; then
      read -r dx dy <<< "$digit"
      adb shell input tap "$dx" "$dy"
      adb shell input keyevent KEYCODE_ENTER
      for attempt in {1..8}; do
        capture ime-edited
        observed=$(cpu_value ime-edited)
        if [[ $observed == 2 ]]; then break; fi
        sleep 1
      done
      [[ $observed == 2 ]] || { echo "System keyboard digit did not reach the CPU field" >&2; exit 1; }
      ime_status=system-keyboard-tap-2-read-back
    fi
  fi
fi
adb shell input keyevent KEYCODE_ENTER
adb shell dumpsys window > evidence/window.txt
grep -F 'mCurrentFocus=' evidence/window.txt | grep -F "$activity"
adb shell pidof "$PACKAGE"
jq -n --arg device "$AVD_DEVICE" --arg profile "$PROFILE" --arg sha256 "$installed_sha" \
  --argjson width "$width" --argjson height "$height" --argjson smallestWidthDp "$smallest" \
  --argjson before "$before" --argjson after "$after" --arg ime "$ime_status" \
  '{device:$device,profile:$profile,installedApkSha256:$sha256,$width,$height,$smallestWidthDp,
     input:{method:"adb-injected-key-events",screen:"English setup",field:"Phone CPU",before:$before,typed:"4",observed:$after},
     systemKeyboard:$ime}' > evidence/receipt.json
cat evidence/receipt.json >> "$GITHUB_STEP_SUMMARY"
