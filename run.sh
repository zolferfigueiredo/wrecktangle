#!/bin/sh
# Makes a debug build of whatever changed and (re)starts Wrecktangle: the
# Linux run.bat. Pass --foreground to keep it in this terminal, with its
# output, until Ctrl+C.
set -e
cd "$(dirname "$0")"

command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

# A debug build recompiles only what changed, in seconds; the install script
# makes the optimized release.
cargo build

# Only one Wrecktangle runs at a time (a second start just opens Settings in
# the first), so the running one quits first. Quitting also puts back the
# Xfce shortcuts it took over.
if pgrep -x wrecktangle >/dev/null; then
    target/debug/wrecktangle --quit
    tries=0
    while pgrep -x wrecktangle >/dev/null && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
    if pgrep -x wrecktangle >/dev/null; then
        # SIGTERM quits the same way, Xfce shortcuts included.
        pkill -x wrecktangle || true
        sleep 1
    fi
fi

if [ "$1" = "--foreground" ]; then
    exec target/debug/wrecktangle
fi

# Its own session keeps it running after this terminal closes.
log="target/wrecktangle.log"
setsid -f target/debug/wrecktangle >"$log" 2>&1 </dev/null
echo "Wrecktangle is running (log: $log)."
