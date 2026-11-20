#!/bin/sh
set -eu
package=$(mktemp -d)
trap 'rm -rf "$package"' EXIT
chown _apt "$package"
cd "$package"
apt-get -o Acquire::Retries=4 download yaru-theme-gtk
dpkg-deb --extract yaru-theme-gtk_*.deb yaru
install -d /usr/share/themes/Yaru-blue
cp -R yaru/usr/share/themes/Yaru-blue/gtk-3.0 /usr/share/themes/Yaru-blue/
cp -R yaru/usr/share/doc/yaru-theme-gtk /usr/share/doc/
glib-compile-schemas --strict /usr/share/glib-2.0/schemas
sed -i '/<theme>/,/<\/theme>/{
  s|<name>Clearlooks</name>|<name>Europium</name>|
  s|<titleLayout>NLIMC</titleLayout>|<titleLayout>LIMC</titleLayout>|
  s|<name>sans</name>|<name>Cantarell</name>|
  s|<size>[89]</size>|<size>11</size>|
}
/<applications>/a\
  <application type="normal"><position><x>center</x><y>center</y></position></application>
' /etc/xdg/openbox/rc.xml
cat >> /etc/X11/app-defaults/XTerm <<'RESOURCES'

*VT100.faceName: DejaVu Sans Mono
*VT100.faceNameDoublesize: Noto Sans Mono CJK SC
*VT100.faceSize: 11
*VT100.renderFont: true
*VT100.background: #1b2537
*VT100.foreground: #e6edf7
*VT100.cursorColor: #a8caf5
*internalBorder: 11
*SimpleMenu.background: #f7f8fa
*SimpleMenu.foreground: #263343
*SimpleMenu.borderColor: #aebac8
*SimpleMenu*leftMargin: 15
*SimpleMenu*rightMargin: 15
RESOURCES
