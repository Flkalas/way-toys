#!/usr/bin/python3
"""Reproduce this user's persistent 3:2 mode setup from the repository root."""
import ast
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import time

ROOT = Path.cwd()
ARTIFACT = ROOT/'docs/experiments/artifacts/2026-11-03-11'
USER = Path.home()


def run(*command):
    with (ARTIFACT/'commands.jsonl').open('a') as log:
        log.write(json.dumps(list(map(str, command)))+'\n')
    result = subprocess.run(list(map(str, command)), capture_output=True, text=True, timeout=20)
    if result.returncode:
        raise RuntimeError(result.stderr or result.stdout)
    return result.stdout


def outputs():
    data = json.loads(run('kscreen-doctor', '--json'))
    result = {}
    for output in data['outputs']:
        if not output['connected'] or not output['enabled']:
            continue
        mode = next(m for m in output['modes'] if m['id'] == output['currentModeId'])
        result[output['name']] = {key:output[key] for key in ('pos','priority','rotation','scale')}
        result[output['name']].update(size=mode['size'], refresh=mode['refreshRate'])
    return result


def atomic_write(path, content, mode):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.display-mode-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if Path(temporary).exists():
            Path(temporary).unlink()


def main():
    assert os.environ.get('XDG_SESSION_TYPE') == 'x11'
    script = USER/'.local/bin/register-3to2-mode'
    config = USER/'.config/display-modes/3to2.json'
    profile = USER/'.xprofile'
    state = USER/'.local/state/display-modes'
    backup = state/'backups'/datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    backup.mkdir(parents=True, mode=0o700)
    for path in (script, config, profile):
        if path.is_symlink():
            raise RuntimeError(f'Preserved existing symlink: {path}')
        if path.exists():
            shutil.copy2(path, backup/path.name)
    shutil.copytree(USER/'.local/share/kscreen', backup/'kscreen', symlinks=True)
    before = outputs()
    (ARTIFACT/'before.json').write_text(json.dumps(before, indent=2)+'\n')
    original = profile.read_text() if profile.exists() else ''
    if '# BEGIN managed 3:2 display mode' in original:
        raise RuntimeError('A managed hook already exists; inspect it before reinstalling')
    if script.exists() and 'Managed 3:2 X11' not in script.read_text():
        raise RuntimeError('Preserved an unrelated existing registration script')
    ast.parse((ARTIFACT/'register-3to2-mode.py').read_text())
    atomic_write(script, (ARTIFACT/'register-3to2-mode.py').read_bytes(), 0o755)
    atomic_write(config, (ARTIFACT/'mode.json').read_bytes(), 0o600)
    registration = run(script)
    hook = '''
# BEGIN managed 3:2 display mode
# SDDM sources this file before Plasma/KScreen restores the saved layout.
if [ -x "$HOME/.local/bin/register-3to2-mode" ]; then
    mkdir -p "$HOME/.local/state/display-modes"
    "$HOME/.local/bin/register-3to2-mode" >> "$HOME/.local/state/display-modes/3to2-login.log" 2>&1 || :
fi
# END managed 3:2 display mode
'''
    profile_mode = stat.S_IMODE(profile.stat().st_mode) if profile.exists() else 0o644
    atomic_write(profile, (original.rstrip()+'\n'+hook).encode(), profile_mode)
    run('sh', '-n', profile)
    position = before['DP-1-1']['pos']
    run('kscreen-doctor', 'output.DP-1-1.mode.2544x1696@60', f'output.DP-1-1.position.{position["x"]},{position["y"]}')
    deadline = time.monotonic()+8
    persisted = []
    while time.monotonic() < deadline:
        persisted = []
        for path in (USER/'.local/share/kscreen').iterdir():
            if not path.is_file():
                continue
            try:
                entries = json.loads(path.read_text())
            except ValueError:
                continue
            if isinstance(entries, list) and any(e.get('metadata',{}).get('name')=='DP-1-1' and e.get('mode',{}).get('size')=={'width':2544,'height':1696} for e in entries):
                persisted.append(path.name)
        if persisted:
            break
        time.sleep(.25)
    if not persisted:
        raise RuntimeError('KScreen did not save the custom mode')
    after = outputs()
    assert after['DP-1-1']['size'] == {'width':2544,'height':1696}, after
    assert after['DP-1-2'] == before['DP-1-2'], (before,after)
    # Run the exact installed login action twice. It only registers the mode;
    # KScreen owns the persistent selected resolution and monitor positions.
    repeated = [run(script),run(script)]
    assert outputs() == after
    report = {'time':datetime.datetime.now().astimezone().isoformat(), 'backup':str(backup),
              'registration':registration.strip(), 'repeat':list(map(str.strip,repeated)),
              'saved_profiles':persisted, 'outputs':after,
              'installed_script_sha256':hashlib.sha256(script.read_bytes()).hexdigest(),
              'login_hook':str(profile), 'relogin_tested':False}
    (ARTIFACT/'applied.json').write_text(json.dumps(report,indent=2)+'\n')
    (backup/'restore-previous-resolution.sh').write_text('#!/bin/sh\nkscreen-doctor output.DP-1-1.mode.3600x2400@60 output.DP-1-1.position.16,1440\n')
    print(json.dumps(report,ensure_ascii=False))


if __name__ == '__main__':
    main()
