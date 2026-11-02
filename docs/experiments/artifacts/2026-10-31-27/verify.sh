set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
cd /home/debian/projects/way-toys
. "$HOME/.cargo/env"
base=/home/debian/waytoys-test-results/paste-27
sha256sum -c "$base/before.sha256"
python3 - <<'PY'
import json
from pathlib import Path
import sqlite3
import subprocess
app=Path('/home/debian/waytoys-test-results/settings-23/app')
old=json.loads(Path('/home/debian/waytoys-test-results/paste-27/settings-before.json').read_text())
new=json.loads((app/'settings.json').read_text())
expected=json.loads(json.dumps(old));expected['enabled']['paste']=True
assert new==expected,(new,expected)
status=json.loads(subprocess.check_output(['target/debug/waytoys','--status']))
assert status['ready'] and status['paste_ready'] and not status['blocked'],status
assert status['error'] is None and status['paste_error'] is None,status
assert status['paths']['paste']==str(app/'paste'),status
assert status['settings']==new,status
history=app/'paste/history.sqlite3'
assert (history.parent.stat().st_mode&0o777)==0o700
assert (history.stat().st_mode&0o777)==0o600
db=sqlite3.connect(f'file:{history}?mode=ro',uri=True)
assert db.execute('PRAGMA quick_check').fetchone()[0]=='ok'
assert db.execute('SELECT COUNT(*) FROM entries WHERE pinned=1').fetchone()[0]==2
print(json.dumps(dict(runtime=status,history_permissions='0700/0600',quick_check='ok',pins=2,only_app_change='enabled.paste=true'),indent=2))
PY
# The XDG paths are sufficient even when HOME is absent. --status is read-only.
env -u HOME XDG_CONFIG_HOME=/tmp/waytoys-paste-xdg-config XDG_DATA_HOME=/tmp/waytoys-paste-xdg-data target/debug/waytoys --status >/dev/null
printf 'XDG without HOME: OK\n'
cargo tree -p waytoys-paste --locked -e normal,build --prefix none > "$base/paste-dependencies.txt"
python3 - <<'PY'
from pathlib import Path
lines=Path('/home/debian/waytoys-test-results/paste-27/paste-dependencies.txt').read_text().splitlines()
assert any(line.startswith('iced_tiny_skia ') for line in lines)
assert not any(line.startswith(('wgpu ','iced_wgpu ')) for line in lines)
print('Compiled dependencies: tiny-skia; no wgpu')
PY
for name in org.waytoys.PastePicker1 org.waytoys.Capture1; do
 test "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner "$name")" = false
done
qdbus6 org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver.GetActive
sha256sum target/debug/waytoys target/debug/waytoys-settings target/debug/waytoys-paste
df -h /home/debian/projects/way-toys
ps -eo pid,ppid,rss,args | grep '[w]aytoys-paste\|[w]l-copy'
