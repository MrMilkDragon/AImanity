#!/usr/bin/env bash
set -euo pipefail

root=/usr/lib/aimanity-workspace/host
work="$HOME/cold-install"
export XDG_RUNTIME_DIR="$work/run" XDG_DATA_HOME="$work/data"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
[[ -r /dev/kvm && -w /dev/kvm ]] || { echo "KVM is unavailable to the installed client" >&2; exit 1; }
openbox --sm-disable > "$work/wm.log" 2>&1 &
wm=$!
aimanity-workspace > "$work/client.log" 2>&1 &
app=$!
trap 'code=$?; if ((code)); then tail -50 "$work/client.log"; fi; kill "$app" "$wm" 2>/dev/null || true' EXIT

window=$(timeout 60 xdotool search --sync --onlyvisible --pid "$app" | head -1)
xdotool windowactivate --sync "$window"
word() {
    local wanted=$1 where attempt
    for attempt in {1..20}; do
        kill -0 "$app"
        import -window "$window" -resize '200%' "$work/current.png"
        where=$(tesseract "$work/current.png" stdout -l eng --psm 11 tsv 2>/dev/null |
            awk -F '\t' -v w="$wanted" '$1==5 && tolower($12)==tolower(w) && (!seen || $8<best) {
                seen=1;best=$8;x=int(($7+$9/2)/2);y=int(($8+$10/2)/2)
            } END {if (seen) print x,y}')
        if [[ -n $where ]]; then read -r x y <<< "$where"; return; fi
        sleep 0.5
    done
    echo "Missing $wanted on the installed client" >&2
    return 1
}
click_word() { word "$1"; xdotool mousemove --window "$window" "$x" "$y" click 1; }
qemu_pids() { ps -u "$(id -u)" -o pid=,comm= | awk '$2 ~ /qemu-system-x86/ {print $1}'; }
click_word English
click_word Continue
timeout 60 xdotool search --sync --onlyvisible --pid "$app" --name '^AImanity$' >/dev/null
test -z "$(qemu_pids)"
test -f "$XDG_DATA_HOME/dev.aimanity.workspace/host/workspace.redb"
# Device Manager is the first icon of the top toolbar on the 1440 x 900 canvas.
read -r width height < <(xwininfo -id "$window" | awk '/Width:/ {w=$2} /Height:/ {h=$2} END {print w,h}')
xdotool mousemove --window "$window" "$((width*1065/1440))" "$((height*30/900))" click 1

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
wmctrl -ic "$(printf '0x%x' "$window")"
deadline=$((SECONDS + 120))
while kill -0 "$app" 2>/dev/null; do (( SECONDS < deadline )) || exit 1; sleep 1; done
wait "$app"
for pid in "${guests[@]}"; do
    if kill -0 "$pid" 2>/dev/null; then echo "Guest $pid remained after exit" >&2; exit 1; fi
done
