set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
cd /home/debian/projects/way-toys
sha256sum -c /home/debian/waytoys-test-results/capture-26/before.sha256
target/debug/waytoys --status
qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Capture1
qdbus6 org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver.GetActive
systemctl --user list-units --all --no-pager 'waytoys*'
pgrep -a wl-copy
python3 scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings
python3 - <<'PY'
import ast,json,subprocess
from pathlib import Path
files=['scripts/install-capture.py','scripts/test-capture-vm.py','scripts/kwin-window-probe.py']
for file in files: ast.parse(Path(file).read_text())
print('Python syntax: OK')
pid=int(subprocess.check_output(['pgrep','-x','wl-copy']).strip())
root=Path('/proc')/str(pid)
status=dict(line.split(':',1) for line in (root/'status').read_text().splitlines())
memory=dict(line.split(':',1) for line in (root/'smaps_rollup').read_text().splitlines()[1:])
print(json.dumps(dict(clipboard_pid=pid,rss_kib=int(status['VmRSS'].split()[0]),pss_kib=int(memory['Pss'].split()[0]))))
PY
sh -n scripts/install-capture.sh
cat /etc/debian_version
uname -a
. "$HOME/.cargo/env"
rustc --version
cargo --version
dpkg-query -W kwin-wayland libqt6core6t64 wl-clipboard python3-dbus
sha256sum target/debug/waytoys-capture target/debug/waytoys target/debug/waytoys-settings
