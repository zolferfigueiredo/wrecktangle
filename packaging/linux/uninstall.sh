#!/bin/sh
# Removes what install.sh put in ~/.local, and the launch at startup entry,
# after putting back the Xfce shortcuts Wrecktangle took over. The settings
# in ~/.config/wrecktangle stay.
set -e

data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
exe="$HOME/.local/bin/wrecktangle"

if [ -x "$exe" ]; then
    # A running copy is asked to quit, which puts its Xfce shortcuts back;
    # with none running, this puts back any that a crash left removed.
    "$exe" --restore-xfce-shortcuts
    tries=0
    while pgrep -x wrecktangle >/dev/null && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
fi

rm -f "$exe" "$data/applications/wrecktangle.desktop" "$config/autostart/wrecktangle.desktop"
rm -f "$data"/icons/hicolor/*/apps/wrecktangle.png "$data/icons/hicolor/scalable/apps/wrecktangle.svg"
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
fi

echo "Wrecktangle is uninstalled. Its settings are still in $config/wrecktangle."
