#!/bin/sh
# Builds an optimized Wrecktangle and installs it for this user into
# ~/.local: the program, its menu entry and its icons. Run it again to
# update; scripts/uninstall-linux.sh removes it. The release's .tar.gz
# installs the same way with its own install.sh.
set -e
cd "$(dirname "$0")/.."

command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

cargo build --release

tree="$(mktemp -d)"
trap 'rm -rf "$tree"' EXIT
packaging/linux/stage.sh target/release/wrecktangle "$tree"
packaging/linux/install.sh "$tree"
