#!/bin/sh
# Lays out the files every Linux package holds, in the tree they install to:
#
#   packaging/linux/stage.sh <wrecktangle binary> <directory>
#
# The .deb and .rpm (nfpm.yaml), the AppImage and the .tar.gz are all made
# from this tree, and install.sh installs from it.
set -e
root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="$1"
out="$2"
if [ -z "$binary" ] || [ -z "$out" ]; then
    echo "usage: stage.sh <wrecktangle binary> <directory>" >&2
    exit 2
fi

install -Dm755 "$binary" "$out/usr/bin/wrecktangle"
install -Dm644 "$root/packaging/linux/wrecktangle.desktop" "$out/usr/share/applications/wrecktangle.desktop"
for size in 32 48 64 256; do
    install -Dm644 "$root/assets/wrecktangle-$size.png" "$out/usr/share/icons/hicolor/${size}x${size}/apps/wrecktangle.png"
done
install -Dm644 "$root/assets/wrecktangle.svg" "$out/usr/share/icons/hicolor/scalable/apps/wrecktangle.svg"
install -Dm644 "$root/LICENSE" "$out/usr/share/doc/wrecktangle/LICENSE"
install -Dm644 "$root/README.md" "$out/usr/share/doc/wrecktangle/README.md"
