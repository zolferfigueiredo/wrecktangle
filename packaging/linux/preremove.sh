#!/bin/sh
# Runs before the .deb or .rpm is removed. A running Wrecktangle quits on
# SIGTERM the way the tray's Quit does, which puts back the Xfce shortcuts
# it took over. An upgrade leaves it running. dpkg passes "remove"; rpm
# passes 0 for a removal and 1 for an upgrade.
case "$1" in
    remove | purge | 0)
        pkill -TERM -x wrecktangle || true
        ;;
esac
exit 0
