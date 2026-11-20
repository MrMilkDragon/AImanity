#!/bin/sh
set -eu
mkdir -p "$1"
archive="$1/debian-archive-keyring.deb"
curl -fsSL --output "$archive" https://deb.debian.org/debian/pool/main/d/debian-archive-keyring/debian-archive-keyring_2025.1_all.deb
printf '%s  %s\n' 9ea7778e443144ca490668737a8ab22dd3e748bb99e805e22ec055abeb3c7fac "$archive" | sha256sum -c - >&2
dpkg-deb -x "$archive" "$1"
printf '%s\n' "$1/usr/share/keyrings/debian-archive-keyring.gpg"
