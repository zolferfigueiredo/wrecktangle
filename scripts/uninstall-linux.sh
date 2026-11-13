#!/bin/sh
# Removes what scripts/install-linux.sh installed, and the launch at startup
# entry, after putting back the Xfce shortcuts Wrecktangle took over. The
# settings in ~/.config/wrecktangle stay.
set -e

data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
exe="$HOME/.local/bin/wrecktangle"
icons="$data/icons/hicolor"

if [ -x "$exe" ]; then
    if pgrep -x wrecktangle >/dev/null; then
        "$exe" --quit
        tries=0
        while pgrep -x wrecktangle >/dev/null && [ "$tries" -lt 50 ]; do
            sleep 0.1
            tries=$((tries + 1))
        done
    fi
    # Xfce shortcuts a copy that crashed or was killed left removed.
    "$exe" --restore-xfce-shortcuts
fi

rm -f "$exe" "$data/applications/wrecktangle.desktop" "$config/autostart/wrecktangle.desktop"
for size in 16 20 24 32 48 64 256; do
    rm -f "$icons/${size}x${size}/apps/wrecktangle.png"
done
rm -f "$icons/scalable/apps/wrecktangle.svg"
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" || true

echo "Wrecktangle is uninstalled. Its settings are still in $config/wrecktangle."
