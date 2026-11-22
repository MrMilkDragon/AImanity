#!/usr/bin/env bash
set -euo pipefail

root=/usr/lib/aimanity/host
work="$HOME/cold-install"
export XDG_RUNTIME_DIR="$HOME/xdg-run"
mkdir -p "$work/host" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
test -r /dev/kvm && test -w /dev/kvm

cat > "$work/request.json" <<'JSON'
{"locale":"en","phone":{"cpus":2,"memory_mb":2048,"storage_gb":16},"desktop":{"cpus":2,"memory_mb":2560,"storage_gb":32}}
JSON
jq --arg home "$work/host" --arg resource "$root" --arg request "$work/request.json" \
    '{home:$home,resource:$resource,request:$request,android:.["android.payload"],images:.["qemu/manifest.json"],resume:false}' \
    "$root/integrity.json" > "$work/host.json"

openbox --sm-disable > "$work/wm.log" 2>&1 &
wm=$!
aimanity "$work/host.json" > "$work/client.log" 2>&1 &
app=$!
cleanup() {
    status=$?
    if (( status )); then
        tail -60 "$work/client.log"
        find "$work/host" -type f \( -name sdk.log -o -name qemu.log -o -name owner.log -o -name boot.log \) \
            -exec grep -Ei 'error|fail|timeout|missing|denied|not found|cannot|panic' {} + | tail -60 || true
        kill "$app" 2>/dev/null || true
    fi
    kill "$wm" 2>/dev/null || true
}
trap cleanup EXIT

# The client assigns its workbench title only after authenticated native services are ready.
deadline=$((SECONDS + 2400))
while (( SECONDS < deadline )); do
    kill -0 "$app"
    window=$( { wmctrl -l 2>/dev/null || true; } | awk '$4 == "AImanity"{print $1; exit}')
    if [[ -n "$window" ]]; then break; fi
    sleep 2
done
test -n "$window"
pids=$(pgrep -u "$(id -u)" -f "^$root/tools/bin/qemu-system-x86_64 ")
test "$(wc -l <<< "$pids")" -eq 2

adb=("$root/tools/bin/adb" -P 15037 -s 127.0.0.1:5565)
phone() { LD_LIBRARY_PATH="$root/tools/lib" "${adb[@]}" "$@"; }
test "$(phone shell id -u | tr -d '\r')" = 0
test "$(phone shell getprop sys.boot_completed | tr -d '\r')" = 1
phone exec-out screencap -p > "$work/phone.png"
test "$(head -c 8 "$work/phone.png" | od -An -tx1 | tr -d ' \n')" = 89504e470d0a1a0a
code=$(curl -sS --max-time 10 -o /dev/null -w '%{http_code}' http://127.0.0.1:17865/desktop/screen)
test "$code" = 401
printf 'Cold workbench opened after %s s; Android frame %s bytes; Linux media endpoint %s\n' \
    "$SECONDS" "$(stat -c %s "$work/phone.png")" "$code"

wmctrl -ic "$window"
deadline=$((SECONDS + 120))
while kill -0 "$app" 2>/dev/null && (( SECONDS < deadline )); do sleep 1; done
if kill -0 "$app" 2>/dev/null; then echo 'Client did not finish shutdown' >&2; exit 1; fi
wait "$app"
for pid in $pids; do
    if kill -0 "$pid" 2>/dev/null; then echo "Guest PID $pid remained after exit" >&2; exit 1; fi
done
