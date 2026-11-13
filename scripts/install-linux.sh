#!/bin/sh
# Builds an optimized Wrecktangle and installs it for this user into
# ~/.local: the program, its menu entry and its icons. Run it again to
# update; scripts/uninstall-linux.sh removes it.
set -e
cd "$(dirname "$0")/.."

command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin"
exe="$bin/wrecktangle"
icons="$data/icons/hicolor"
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/wrecktangle.desktop"

cargo build --release

# A running copy quits first. Quitting puts back the Xfce shortcuts it took
# over; the new copy takes them again when it starts.
if pgrep -x wrecktangle >/dev/null; then
    target/release/wrecktangle --quit
    tries=0
    while pgrep -x wrecktangle >/dev/null && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
fi

install -Dm755 target/release/wrecktangle "$exe"
for size in 16 20 24 32 48 64 256; do
    [ -f "assets/wrecktangle-$size.png" ] || continue
    install -Dm644 "assets/wrecktangle-$size.png" "$icons/${size}x${size}/apps/wrecktangle.png"
done
install -Dm644 assets/wrecktangle.svg "$icons/scalable/apps/wrecktangle.svg"

mkdir -p "$data/applications"
cat >"$data/applications/wrecktangle.desktop" <<ENTRY
[Desktop Entry]
Type=Application
Name=Wrecktangle
Comment=Snap windows from the keyboard
Exec="$exe"
Icon=wrecktangle
Terminal=false
Categories=Utility;
ENTRY

command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$data/applications" || true

# Launch at startup turned on from a development build points at that
# build; it moves to the installed copy.
if [ -f "$autostart" ]; then
    sed -i "s|^Exec=.*|Exec=\"$exe\"|" "$autostart"
fi

setsid -f "$exe" >/dev/null 2>&1 </dev/null
echo "Installed $exe and started it. It is in the menu as Wrecktangle."
