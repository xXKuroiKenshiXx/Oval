#!/bin/sh
# Builds a Void Linux package (.xbps) out of the release .deb. Runs inside a
# Void container:  build.sh <oval.deb> <version> <output dir>
set -eu
deb=$(realpath "$1")
version=$2
out=$(realpath "$3")

xbps-install -Syu xbps > /dev/null
xbps-install -Sy bsdtar > /dev/null

work=$(mktemp -d)
cd "$work"
bsdtar -xf "$deb"
mkdir root
bsdtar -xf data.tar.* -C root
chmod 755 root/usr/bin/oval

xbps-create -A x86_64 \
  -n "oval-${version}_1" \
  -s "Minimal oval video downloader" \
  -l MIT \
  -H https://github.com/xXKuroiKenshiXx/Oval \
  -m "xXKuroiKenshiXx <111004221+xXKuroiKenshiXx@users.noreply.github.com>" \
  -D "libwebkit2gtk41>=0 gtk+3>=0 libsoup3>=0 hicolor-icon-theme>=0" \
  root
mv ./*.xbps "$out/"
