#!/bin/sh
set -eu

DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=4 install -y --no-install-recommends \
  libglib2.0-0t64 libice6 libobrender32v5 libobt2v5 libsm6 libstartup-notification0 \
  libx11-6 libxau6 libxcursor1 libxext6 libxi6 libxinerama1 libxrandr2

package=$(mktemp -d)
trap 'rm -rf "$package"' EXIT
chown _apt "$package"
cd "$package"
apt-get -o Acquire::Retries=4 download openbox
dpkg-deb --extract openbox_*.deb files
install -m 0755 files/usr/bin/openbox files/usr/bin/obxprop /usr/bin/
install -d /etc/xdg/openbox
install -m 0644 files/etc/xdg/openbox/rc.xml files/etc/xdg/openbox/menu.xml /etc/xdg/openbox/
cp -R files/usr/share/locale files/usr/share/themes /usr/share/
cp -R files/usr/share/doc/openbox /usr/share/doc/
openbox --version
