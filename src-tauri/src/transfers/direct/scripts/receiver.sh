#!/bin/sh
# Called by the generated, fully quoted command.sh; stdin is file data only.
set -f
umask 077
destination=$1
test ! -e "$destination" && test ! -L "$destination" || exit 73
set -C
exec cat > "$destination"
