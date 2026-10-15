#!/bin/sh
# Positional arguments preserve spaces, quotes and trailing newlines in paths.
set -f
source=$1
shift
test -f "$source" && test ! -L "$source" || exit 66
exec "$@" < "$source"
