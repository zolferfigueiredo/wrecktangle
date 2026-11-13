#!/bin/sh
# Removes what scripts/install-linux.sh installed. See
# packaging/linux/uninstall.sh, which the release's .tar.gz carries too.
exec "$(dirname "$0")/../packaging/linux/uninstall.sh"
