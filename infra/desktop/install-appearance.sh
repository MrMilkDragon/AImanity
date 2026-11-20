#!/bin/sh
set -eu
cd /usr/share/themes/Breeze-ob/openbox-3
install -m 0644 -t /usr/share/themes/Europium/openbox-3 close.xbm iconify.xbm max.xbm max_toggled.xbm
sed -i '/<theme>/,/<\/theme>/{
  s|<name>Clearlooks</name>|<name>Europium</name>|
  s|<titleLayout>NLIMC</titleLayout>|<titleLayout>LIMC</titleLayout>|
  s|<size>[89]</size>|<size>10</size>|
}' /etc/xdg/openbox/rc.xml
cat >> /etc/X11/app-defaults/XTerm <<'RESOURCES'

*VT100.faceName: DejaVu Sans Mono
*VT100.faceNameDoublesize: Noto Sans Mono CJK SC
*VT100.faceSize: 11
*VT100.renderFont: true
*VT100.background: #1b2537
*VT100.foreground: #e6edf7
*VT100.cursorColor: #a8caf5
*internalBorder: 11
RESOURCES
