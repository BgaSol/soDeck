#!/bin/sh
# Xvfb supplies pixels; a window manager must acknowledge minimize/unminimize.
set -eu
dbus-update-activation-environment DISPLAY
openbox --sm-disable >/dev/null 2>&1 &
desktop_pid=$!
trap 'kill "$desktop_pid" 2>/dev/null || true' EXIT INT TERM
attempt=0
until xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; do
  if ! kill -0 "$desktop_pid" 2>/dev/null || [ "$attempt" -ge 50 ]; then
    echo 'Native package verification window manager did not start' >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.1
done
"$@"
