#!/usr/bin/env bash
set -euo pipefail
activity="$PACKAGE/dev.aimanity.mobile.WorkbenchActivity"
mkdir -p evidence
trap 'adb logcat -d -t 2500 > evidence/logcat.txt || true; adb shell dumpsys activity activities > evidence/activities.txt || true; adb exec-out screencap -p > evidence/last.png || true' EXIT

printf '%s  %s\n' "$APK_SHA256" "$APK" | sha256sum --check --strict
adb shell input keyevent KEYCODE_WAKEUP
adb shell wm dismiss-keyguard
adb shell settings put secure immersive_mode_confirmations confirmed
adb shell settings put secure show_ime_with_hard_keyboard 1
adb shell cmd connectivity airplane-mode enable
adb shell svc wifi disable
adb shell svc data disable
[[ $(adb shell settings get global airplane_mode_on | tr -d '\r') == 1 ]]
for attempt in {1..20}; do
  adb shell dumpsys connectivity | grep -F 'Active default network: none' > evidence/offline.txt && break
  sleep 0.25
done
test -s evidence/offline.txt
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
  awk -F '\t' -v word="$2" -v row="${3:-}" '
    $1 == 5 && index(tolower($12),tolower(word)) && (row == "" || ($8 >= row-$10 && $8 <= row+$10)) && (!found || $8 < y) {
      x=$7; y=$8; w=$9; h=$10; found=1
    }
    END { if (!found) exit 1; print x,y,w,h }
  ' "evidence/$1.tsv"
}
wait_word() {
  for attempt in {1..20}; do
    capture "$1"
    if word_box "$1" "$2" "${3:-}" >/dev/null; then return 0; fi
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
hide_keyboard() {
  local info="evidence/$1-input-method.txt"
  sleep 0.5
  adb shell dumpsys input_method > "$info"
  if grep -qE 'mInputShown=true|inputShown=true' "$info"; then
    # A landscape phone keyboard can cover the edited control after Enter.
    adb shell input keyevent KEYCODE_BACK
    for attempt in {1..10}; do
      sleep 0.4
      adb shell dumpsys input_method > "$info"
      if ! grep -qE 'mInputShown=true|inputShown=true' "$info"; then return; fi
    done
    echo "System keyboard still covers the app after Back" >&2
    return 1
  fi
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
rustc --edition 2024 -O scripts/mobile-button-row.rs -o "$RUNNER_TEMP/mobile-button-row"
button_y=$(convert evidence/selected.png -colorspace Gray -threshold '25%' -depth 8 gray:- | \
  "$RUNNER_TEMP/mobile-button-row" "$width" "$height")
adb shell input tap "$((width/2))" "$button_y"
wait_word workspace Message
read -r _ resting_y _ _ < <(word_box workspace Message)
adb shell dumpsys package "$PACKAGE" > evidence/package.txt
grep -F 'android.permission.RECORD_AUDIO: granted=false' evidence/package.txt

for attempt in {1..8}; do
  capture composer
  if word_box composer Message >/dev/null; then tap_word composer Message; fi
  adb shell dumpsys input_method > evidence/composer-focus.txt
  if grep -Fq 'connection=dev.aimanity.mobile.KeyboardConnection@' evidence/composer-focus.txt; then break; fi
  sleep 0.4
done
grep -Fq 'connection=dev.aimanity.mobile.KeyboardConnection@' evidence/composer-focus.txt
adb shell 'input text O && input keyevent KEYCODE_R KEYCODE_B KEYCODE_I KEYCODE_T'
hide_keyboard hardware
wait_word typed Orbit "$resting_y"
tap_word typed Orbit
adb shell input keyevent KEYCODE_MOVE_END
key_location() {
  awk -F '\t' -v key="$2" -v top="$keyboard_top" '
    $1 == 5 && $8 > top && $12 ~ key {print int($7+$9/2),int($8+$10/2); exit}
  ' "evidence/$1.tsv"
}
symbols=''
for attempt in {1..12}; do
  sleep 0.8
  adb shell dumpsys input_method > evidence/input-method.txt
  if ! grep -qE 'mInputShown=true|inputShown=true' evidence/input-method.txt; then continue; fi
  # During Gboard's entrance Android reports the IME shown before the
  # keyboard covers its final area or provides any actual key targets.
  keyboard_top=$(sed -nE 's/.*visibleTopInsets=([0-9]+) touchableInsets=.*/\1/p' evidence/input-method.txt | head -1)
  if [[ ! $keyboard_top =~ ^[0-9]+$ ]] || (( keyboard_top == 0 || keyboard_top >= height )); then
    keyboard_top=$((height/2))
  fi
  capture keyboard
  symbols=$(key_location keyboard '123')
  if [[ -n $symbols ]] && awk -F '\t' -v top="$keyboard_top" '$1 == 5 && tolower($12) ~ /^orbit/ && $8 + $10 <= top { visible=1 } END { exit !visible }' evidence/keyboard.tsv; then break; fi
  symbols=''
done
# Start with the symbols layout where a tap on the numeral commits it.
# A numeral printed as a secondary hint on a letter needs a long press.
[[ -n $symbols ]] || { echo "Composer text and the keyboard must be visible together" >&2; exit 1; }
for attempt in {1..8}; do
  if [[ -n $symbols ]]; then
    read -r sx sy <<< "$symbols"
    adb shell input tap "$sx" "$sy"
  fi
  sleep 0.8
  capture symbols
  symbols=$(key_location symbols '123')
  if [[ -n $symbols ]]; then continue; fi
  [[ -n $(key_location symbols '^ABC$') ]] || continue
  digit=$(awk -F '\t' -v top="$keyboard_top" -v width="$width" '
    $1 == 5 && $8 > top && $12 ~ /^[1-9]$/ {
      n=$12+0; x=$7+$9/2; y=$8+$10/2
      for (a in cx) if (a!=n && cy[a]-y<8 && y-cy[a]<8) {
        pitch=(x-cx[a])/(n-a)
        if (pitch>width/16 && pitch<width/8) {
          print int(x+(2-n)*pitch),int(y); exit
        }
      }
      cx[n]=x; cy[n]=y
    }' evidence/symbols.tsv)
  if [[ -n $digit ]]; then break; fi
done
[[ -n ${digit:-} ]] || { echo "System IME did not expose its 2 key" >&2; exit 1; }
read -r dx dy <<< "$digit"
adb shell input tap "$dx" "$dy"
hide_keyboard soft
wait_word composed Orbit2 "$resting_y"

adb shell input keyevent KEYCODE_HOME
adb shell am start -W -n "$activity" > evidence/resume.txt
wait_word resumed Orbit2 "$resting_y"
adb shell dumpsys input_method > evidence/resumed-input-method.txt
if grep -qE 'mInputShown=true|inputShown=true' evidence/resumed-input-method.txt; then
  echo "Returning to the workspace reopened the dismissed keyboard" >&2
  exit 1
fi
adb shell am force-stop "$PACKAGE"
adb shell am start -W -n "$activity" > evidence/reopen.txt
wait_word reopened Message
adb shell dumpsys window > evidence/window.txt
grep -F 'mCurrentFocus=' evidence/window.txt | grep -F "$activity"
jq -n --arg device "$AVD_DEVICE" --arg profile "$PROFILE" --arg sha256 "$installed_sha" \
  --argjson width "$width" --argjson height "$height" --argjson sw "$smallest" \
  '{device:$device,profile:$profile,installedApkSha256:$sha256,$width,$height,smallestWidthDp:$sw,
    firstLaunch:"zero-device workspace",input:"Orbit2",backgroundResume:"draft retained",
    coldReopen:"English workspace",microphone:"unrequested"}' > evidence/receipt.json
cat evidence/receipt.json >> "$GITHUB_STEP_SUMMARY"
