#!/usr/bin/python3
"""Only used in the isolated test image; normal calls execute real rsync."""
import os
import pathlib
import sys

args = sys.argv[1:]
mode_file = pathlib.Path('/home/ops/rsync-test-mode')
mode = mode_file.read_text().strip() if mode_file.exists() else ''
if '--version' not in args:
    if mode == 'fail':
        sys.stderr.write('fixture disk full\n' + 'diagnostic\n' * 20000)
        raise SystemExit(23)
    if mode == 'slow' and '--server' not in args:
        args.insert(0, '--bwlimit=256')
os.execv('/usr/bin/rsync', ['rsync', *args])
