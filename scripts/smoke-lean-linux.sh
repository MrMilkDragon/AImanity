#!/usr/bin/env bash
set -euo pipefail

out=$1
arch=$2
desktop=$(jq -er '.images.desktop.ref' "$out/images.receipt.json")
tar -xOf "$out/images.payload" images.tar.xz | xz -dc | docker image load --platform "linux/$arch"
container=$(docker run -d --network none --init --shm-size=256m "$desktop")
trap 'docker logs "$container"; docker rm -f "$container" >/dev/null' EXIT

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
docker exec -d -u ai "$container" sh -ec 'export DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/ai-runtime/dbus-address); exec /usr/local/bin/ai-phone-browser "data:text/html,<title>AImanity Chromium</title>"'
docker exec -d -u ai "$container" sh -ec 'export DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/ai-runtime/dbus-address); exec xterm -title "AImanity Terminal"'
docker exec -d -u ai "$container" sh -ec 'export DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/ai-runtime/dbus-address); exec localc --norestore'
docker exec -u ai "$container" timeout 35 sh -ec '
  until curl --fail --silent http://127.0.0.1:9222/json/list | grep -q "AImanity Chromium"; do sleep 1; done
  xdotool search --sync --onlyvisible --name "^AImanity Terminal$"
  xdotool search --sync --onlyvisible --name "LibreOffice Calc"
  ffmpeg -hide_banner -loglevel error -y -f x11grab -i :0 -frames:v 1 /tmp/desktop.png
'
docker cp "$container:/tmp/desktop-menu.png" "$out/desktop-menu.png"
docker cp "$container:/tmp/desktop.png" "$out/desktop.png"
