#!/usr/bin/env bash
set -euo pipefail

out=$1
arch=$2
receipt="$out/images.receipt.json"
test "$(jq -er .source "$receipt")" = "$BERYLLIUM_SOURCE"
test "$(jq -er .architecture "$receipt")" = "$arch"
desktop=$(jq -er '.images.desktop.ref' "$receipt")
jq '{source, architecture, bytes, archive_sha256, desktop:.images.desktop}' "$receipt" > "$out/desktop-attestation.json"
cat "$out/desktop-attestation.json"
tar -xOf "$out/images.payload" images.tar.xz | xz -dc | docker image load --platform "linux/$arch"
container=$(docker run -d --network none --init --shm-size=256m "$desktop")
report() {
  local status=$?
  if [ "$status" -ne 0 ]; then
    docker exec -u ai "$container" sh -c 'for app in browser terminal calc; do echo "--- $app ---"; cat "/tmp/qa-$app.log" 2>/dev/null; done; echo "--- windows ---"; xwininfo -root -tree; echo "--- CDP ---"; curl --noproxy "*" -sS http://127.0.0.1:9222/json/list' || true
    docker logs "$container"
  fi
  docker rm -f "$container" >/dev/null
}
trap report EXIT

timeout 30 bash -c 'until docker exec -u ai "$1" sh -ec "xdpyinfo >/dev/null && pactl info >/dev/null && test -s /tmp/ai-runtime/dbus-address"; do sleep 1; done' sh "$container"
docker exec -u ai "$container" sh -ec '
  export DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/ai-runtime/dbus-address)
  /usr/local/bin/desktop-bridge --version
  test "$(dpkg-query -W -f="\${Status}" fonts-noto-cjk)" = "install ok installed"
  ! command -v dbus-launch
  test "$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.freedesktop.Notifications)" = "(true,)"
  notify-send "AImanity desktop check" "The shared Linux desktop is running."
  for language in eng chi_sim; do tesseract --list-langs | grep -qx "$language"; done
  xdotool mousemove 600 320 click 3
  ffmpeg -hide_banner -loglevel error -y -f x11grab -i :0 -frames:v 1 /tmp/desktop-menu.png
  xdotool key Escape
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i sine=frequency=440:duration=0.15 /tmp/lean-tone.wav
  paplay /tmp/lean-tone.wav
'
docker cp "$container:/tmp/desktop-menu.png" "$out/desktop-menu.png"
launch() {
  docker exec -d -u ai "$container" sh -ec 'export DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/ai-runtime/dbus-address); log=$1; shift; exec "$@" >"/tmp/qa-$log.log" 2>&1' sh "$@"
}
launch browser /usr/local/bin/ai-phone-browser 'data:text/html,%3Ctitle%3EAImanity%20Chromium%3C/title%3E'
launch terminal xterm -title 'AImanity Terminal'
launch calc localc --norestore
echo 'Checking Terminal window'
docker exec -u ai "$container" timeout 25 xdotool search --sync --onlyvisible --name '^AImanity Terminal$'
echo 'Checking LibreOffice Calc window'
docker exec -u ai "$container" timeout 25 xdotool search --sync --onlyvisible --name 'LibreOffice Calc'
echo 'Checking Chromium offline CDP page'
docker exec -u ai "$container" timeout 25 sh -ec '
  until curl --noproxy "*" --max-time 2 --fail --silent http://127.0.0.1:9222/json/list | grep -q "AImanity Chromium"; do sleep 1; done
'
docker exec -u ai "$container" ffmpeg -hide_banner -loglevel error -y -f x11grab -i :0 -frames:v 1 /tmp/desktop.png
docker cp "$container:/tmp/desktop.png" "$out/desktop.png"
