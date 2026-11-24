#!/usr/bin/env bash
set -euo pipefail

root=/usr/lib/aimanity-workspace/host
work="$HOME/cold-install"
if [[ $1 == guests ]]; then work="$HOME/optional-guests"; fi
export XDG_RUNTIME_DIR="$work/run" XDG_DATA_HOME="$work/data"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
openbox --sm-disable > "$work/wm.log" 2>&1 &
wm=$!
start_client() {
    aimanity-workspace >> "$work/client.log" 2>&1 &
    app=$!
    window=$(timeout 60 xdotool search --sync --onlyvisible --pid "$app" | head -1)
    xdotool windowactivate --sync "$window"
}
close_client() {
    wmctrl -ic "$(printf '0x%x' "$window")"
    timeout 120 tail --pid="$app" -f /dev/null
    wait "$app"
}
reopen() {
    local database="$XDG_DATA_HOME/dev.aimanity.workspace/host/workspace.redb"
    local identity start_ms
    identity=$(stat -c '%d:%i' "$database")
    start_ms=$(date +%s%3N)
    start_client
    capture_workbench reopened-workbench
    echo "$(( $(date +%s%3N) - start_ms ))" > "$work/reopen-ms.txt"
    test "$(stat -c '%d:%i' "$database")" = "$identity"
    close_client
}
start_client
trap 'code=$?; if ((code)); then tail -50 "$work/client.log"; fi; kill "$app" "$wm" 2>/dev/null || true' EXIT

word() {
    local wanted=$1 where attempt previous=""
    for attempt in {1..20}; do
        kill -0 "$app"
        import -window "$window" -resize '200%' "$work/current.png"
        where=$(tesseract "$work/current.png" stdout -l eng --psm 11 tsv 2>/dev/null |
            awk -F '\t' -v w="$wanted" '$1==5 { label=$12; sub(/[[:punct:]]+$/, "", label); if (tolower(label)!=tolower(w) || (seen && $8>=best)) next;
                seen=1;best=$8;x=int(($7+$9/2)/2);y=int(($8+$10/2)/2)
            } END {if (seen) print x,y}')
        if [[ -n $where && $where == "$previous" ]]; then read -r x y <<< "$where"; return; fi
        previous=$where
        sleep 0.5
    done
    echo "Missing $wanted on the installed client" >&2
    return 1
}
click_word() { word "$1"; xdotool mousemove --window "$window" "$x" "$y" sleep 0.1 mousedown 1 sleep 0.1 mouseup 1; }
qemu_pids() { ps -u "$(id -u)" -o pid=,comm= | awk '$2 ~ /qemu-system-x86/ {print $1}'; }
capture_workbench() {
    local output=$1 w h ink
    window=$(timeout 60 xdotool search --sync --onlyvisible --pid "$app" --name '^AImanity$' | head -1)
    for _ in {1..100}; do
        import -window "$window" "$work/current.png"
        read -r w h < <(identify -format '%w %h\n' "$work/current.png")
        if (( w > h )); then
            ink=$(convert "$work/current.png" -crop "$((w*14/100))x$((h*8/100))+$((w*80/100))+$((h*88/100))" +repage -colorspace Gray -format '%[fx:minima<0.58]' info:)
            if [[ "$ink" == 1 ]]; then
                test "$(cat "$XDG_DATA_HOME/dev.aimanity.workspace/host/user/native-locale")" = en
                cp "$work/current.png" "$work/$output.png"
                return
            fi
        fi
        sleep 0.15
    done
    echo "Conversation controls did not appear on the installed workbench" >&2
    return 1
}

button_y=""
for _ in {1..80}; do
    import -window "$window" "$work/current.png"
    read -r width height < <(identify -format '%w %h\n' "$work/current.png")
    if button_y=$(convert "$work/current.png" -colorspace Gray -threshold '25%' -depth 8 gray:- | /action-row "$width" "$height" 2>/dev/null); then break; fi
    sleep 0.15
done
test -n "$button_y"
cp "$work/current.png" "$work/language.png"
xdotool mousemove --window "$window" "$((width * 60 / 100))" "$((height * 32 / 100))" sleep 0.1 click 1
xdotool mousemove --window "$window" "$((width / 2))" "$button_y" sleep 0.1 click 1
capture_workbench first-workbench
test -z "$(qemu_pids)"
test -f "$XDG_DATA_HOME/dev.aimanity.workspace/host/workspace.redb"
if [[ $1 == workbench ]]; then
    close_client
    reopen
    exit 0
fi
[[ -r /dev/kvm && -w /dev/kvm ]]
eval "$(xdotool getwindowgeometry --shell "$window")"
xdotool mousemove --window "$window" "$((WIDTH * 739 / 1000))" "$((HEIGHT / 30))"
word Share

for kind in phone desktop; do
    click_word Add
    word Name
    xdotool type --clearmodifiers "QA-$kind"
    if [[ $kind == desktop ]]; then
        click_word Android
        click_word Linux
    fi
    click_word installation
    deadline=$((SECONDS + 1500))
    until grep -Eq "native_metric first_frame .* kind=$kind$" "$work/client.log"; do
        kill -0 "$app"
        (( SECONDS < deadline )) || { echo "No $kind frame reached the installed client" >&2; exit 1; }
        sleep 1
    done
done
mapfile -t guests < <(qemu_pids)
(( ${#guests[@]} == 2 ))
phone() { LD_LIBRARY_PATH="$root/tools/lib" "$root/tools/bin/adb" -P 16037 "$@"; }
test "$(phone shell id -u | tr -d '\r')" = 0
phone exec-out screencap -p > "$work/phone.png"
test "$(head -c 8 "$work/phone.png" | od -An -tx1 | tr -d ' \n')" = 89504e470d0a1a0a
grep 'native_metric first_frame ' "$work/client.log"
close_client
for pid in "${guests[@]}"; do
    if kill -0 "$pid" 2>/dev/null; then echo "Guest $pid remained after exit" >&2; exit 1; fi
done
reopen
