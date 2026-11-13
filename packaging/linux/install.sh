#!/bin/sh
# Installs Wrecktangle for this user into ~/.local: the program, its menu
# entry and its icons, then starts it. It installs from the tree next to it
# (the unpacked .tar.gz), or from the tree given as its argument (what
# scripts/install-linux.sh builds). Run it again to update; uninstall.sh
# removes it.
set -e
tree="${1:-$(cd "$(dirname "$0")" && pwd)}"
if [ ! -x "$tree/usr/bin/wrecktangle" ]; then
    echo "No usr/bin/wrecktangle in $tree." >&2
    exit 1
fi

data="${XDG_DATA_HOME:-$HOME/.local/share}"
exe="$HOME/.local/bin/wrecktangle"
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/wrecktangle.desktop"

# A running copy quits first. Quitting puts back the Xfce shortcuts it took
# over; the new copy takes them again when it starts.
if pgrep -x wrecktangle >/dev/null; then
    "$tree/usr/bin/wrecktangle" --quit
    tries=0
    while pgrep -x wrecktangle >/dev/null && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
fi

install -Dm755 "$tree/usr/bin/wrecktangle" "$exe"
(cd "$tree/usr/share" && find icons -type f) | while read -r file; do
    install -Dm644 "$tree/usr/share/$file" "$data/$file"
done
# ~/.local/bin is not on every session's PATH, so the menu entry names the
# program by its full path.
mkdir -p "$data/applications"
sed "s|^Exec=.*|Exec=\"$exe\"|" "$tree/usr/share/applications/wrecktangle.desktop" \
    >"$data/applications/wrecktangle.desktop"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q "$data/applications" || true
fi

# Launch at startup turned on from another copy points at that copy; it
# moves to this one.
if [ -f "$autostart" ]; then
    sed -i "s|^Exec=.*|Exec=\"$exe\"|" "$autostart"
fi

setsid -f "$exe" >/dev/null 2>&1 </dev/null
echo "Installed $exe and started it. It is in the menu as Wrecktangle."
