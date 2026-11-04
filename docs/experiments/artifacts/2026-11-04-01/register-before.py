#!/usr/bin/python3
"""Managed 3:2 X11 mode registration; KScreen restores the saved mode selection.

Run before Plasma through ~/.xprofile. Only register the mode for the monitor
with the configured EDID hash. This does not switch the active resolution.
"""
import json
import hashlib
import os
from pathlib import Path
import re
import subprocess


def main():
    if os.environ.get('XDG_SESSION_TYPE', 'x11') != 'x11' or not os.environ.get('DISPLAY'):
        return
    config = json.loads((Path.home()/'.config/display-modes/3to2.json').read_text())
    query = subprocess.check_output(['xrandr', '--prop'], text=True, timeout=10)
    blocks = re.split(r'(?=^\S+ (?:connected|disconnected)\b)', query, flags=re.M)
    for block in blocks:
        header = re.match(r'^(\S+) connected\b', block)
        if not header:
            continue
        edid = re.search(r'^\s+EDID:\s*\n((?:\s+[0-9a-fA-F]{32}\s*\n)+)', block, re.M)
        if not edid:
            continue
        digest = hashlib.sha256(bytes.fromhex(''.join(edid.group(1).split()))).hexdigest()
        if digest != config['edid_sha256']:
            continue
        output = header.group(1)
        mode = config['mode']
        if re.search(r'^\s+'+re.escape(mode)+r'\s', block, re.M):
            print(f'{output}: {mode} already registered')
            return
        created = subprocess.run(['xrandr', '--newmode', mode, *config['timing']], capture_output=True, text=True, timeout=10)
        if created.returncode and 'BadName' not in created.stderr:
            raise RuntimeError(created.stderr.strip())
        # BadName means the server already knows the mode, possibly detached
        # after a reconnect. Attach it to the identified output again.
        subprocess.run(['xrandr', '--addmode', output, mode], check=True, timeout=10)
        print(f'{output}: registered {mode}')
        return
    print('Configured 3:2 monitor is not connected; skipped')


if __name__ == '__main__':
    main()
