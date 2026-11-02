#!/usr/bin/env python3
"""Install the user-local KDE capture desktop entry and optional chosen shortcut.

No compositor security bypass is used. KWin matches the executable to the
desktop entry's X-KDE-DBUS-Restricted-Interfaces declaration.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

MARKER = '# Managed by Waytoys Capture\n'
COMPONENT = 'org.waytoys.Capture.desktop'
KEY = 0x10000000 | 0x02000000 | ord('S')  # Qt Meta + Shift + S
ACTION = [COMPONENT, '_launch', 'Waytoys Capture', 'Capture a region']

def absolute_env(name, default):
    value = Path(os.environ.get(name) or default)
    if not value.is_absolute():
        raise RuntimeError(f'{name} must be absolute')
    return value

def atomic(path, data, mode=0o600):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise RuntimeError(f'Refusing symlink: {path}')
    fd, temporary = tempfile.mkstemp(prefix='.waytoys-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

def shortcuts(restore=False):
    import dbus
    api = dbus.Interface(dbus.SessionBus().get_object('org.kde.kglobalaccel', '/kglobalaccel'), 'org.kde.KGlobalAccel')
    backup = absolute_env('XDG_STATE_HOME', Path.home()/'.local/state')/'waytoys/capture-shortcut.json'
    def keys(action):
        return [int(x) for x in api.shortcut(dbus.Array(action, signature='s'))]
    def set_keys(action, values):
        api.setForeignShortcut(dbus.Array(action, signature='s'), dbus.Array(values, signature='i'))
    if restore:
        records = json.loads(backup.read_text())
        if keys(ACTION) != [KEY]:
            raise RuntimeError('Waytoys shortcut was changed externally; preserved')
        # Check every affected action before changing any of them.
        for record in records:
            if keys(record['action']) != record['installed']:
                raise RuntimeError('A previous shortcut was changed externally; preserved')
        set_keys(ACTION, [])
        for record in records:
            set_keys(record['action'], record['before'])
        backup.rename(backup.with_name(f'capture-shortcut-restored-{time.time_ns()}.json'))
        print('Restored the previous Meta+Shift+S assignment; desktop entry retained')
        return
    owners = api.getGlobalShortcutsByKey(KEY)
    records = []
    for info in owners:
        component, action = str(info[2]), str(info[0])
        if component == COMPONENT:
            api.setShortcut(dbus.Array(ACTION, signature='s'), dbus.Array([KEY], signature='i'), dbus.UInt32(6))
            print('Meta+Shift+S already assigned to Waytoys Capture')
            return
        if component != 'org.kde.spectacle.desktop' or action != '_launch':
            raise RuntimeError(f'Meta+Shift+S belongs to {component}/{action}; preserved')
        action_id = [component, action, str(info[3]), str(info[1])]
        before = keys(action_id)
        records.append({'action': action_id, 'before': before, 'installed': [k for k in before if k != KEY]})
    if backup.exists():
        raise RuntimeError(f'Previous shortcut backup already exists: {backup}; restore or inspect it first')
    atomic(backup, json.dumps(records, ensure_ascii=False, indent=2)+'\n')
    try:
        for record in records:
            set_keys(record['action'], record['installed'])
        api.doRegister(dbus.Array(ACTION, signature='s'))
        # SetPresent (2) | NoAutoloading (4): both persist and activate now.
        assigned = api.setShortcut(dbus.Array(ACTION, signature='s'), dbus.Array([KEY], signature='i'), dbus.UInt32(6))
        if [int(x) for x in assigned] != [KEY]:
            raise RuntimeError('KDE did not accept Meta+Shift+S')
        print(f'Meta+Shift+S assigned to Waytoys Capture; previous keys saved in {backup}')
    except Exception:
        set_keys(ACTION, [])
        for record in records:
            if keys(record['action']) == record['installed']:
                set_keys(record['action'], record['before'])
        raise

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1]/'target/debug/waytoys-capture')
    parser.add_argument('--shortcut', action='store_true', help='Assign Meta+Shift+S; retain Spectacle PrintScreen')
    parser.add_argument('--restore-shortcut', action='store_true')
    args = parser.parse_args()
    if args.restore_shortcut:
        shortcuts(restore=True)
        return
    binary = args.binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError('Capture binary is not executable')
    if any(c in str(binary) for c in '\n\r\x00'):
        raise RuntimeError('Unsupported executable path')
    # Desktop Exec has its own quoting rules; no shell is involved.
    executable = str(binary).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
    executable = executable.replace('\\', '\\\\')
    entry = absolute_env('XDG_DATA_HOME', Path.home()/'.local/share')/'applications'/COMPONENT
    if entry.exists() and not entry.read_text().startswith(MARKER):
        raise RuntimeError(f'Unmanaged desktop entry preserved: {entry}')
    content = MARKER + f'''[Desktop Entry]
Type=Application
Name=Waytoys Capture
Name[ko]=Waytoys 영역 캡처
Comment=Copy a selected screen region to the clipboard
Comment[ko]=선택한 화면 영역을 클립보드에 복사합니다
Exec="{executable}"
Icon=camera-photo
Terminal=false
Categories=Utility;
StartupNotify=false
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
'''
    atomic(entry, content, 0o644)
    environment = dict(os.environ)
    environment.setdefault('XDG_MENU_PREFIX', 'plasma-')
    subprocess.run(['kbuildsycoca6', '--noincremental'], check=True, env=environment)
    if args.shortcut:
        shortcuts()
    print(f'Installed {entry} -> {binary}')

if __name__ == '__main__':
    main()
