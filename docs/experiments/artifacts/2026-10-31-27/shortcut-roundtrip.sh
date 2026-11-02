set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
cd /home/debian/projects/way-toys
python3 - <<'PY'
import dbus
import json
import subprocess
api=dbus.Interface(dbus.SessionBus().get_object('org.kde.kglobalaccel','/kglobalaccel'),'org.kde.KGlobalAccel')
def owners(key):return [(str(i[2]),str(i[0])) for i in api.getGlobalShortcutsByKey(key)]
unchanged={key:owners(key) for key in [301989971,16777225]}
before=owners(268435542)
assert before==[('org.waytoys.Paste.desktop','_launch')],before
subprocess.run(['sh','scripts/install-paste.sh','--restore-shortcut'],check=True)
restored=owners(268435542)
assert restored==[('plasmashell','show-on-mouse-pos')],restored
subprocess.run(['sh','scripts/install-paste.sh','--shortcut'],check=True)
subprocess.run(['sh','scripts/install-paste.sh','--shortcut'],check=True)
after=owners(268435542)
assert after==before,(before,after)
assert unchanged=={key:owners(key) for key in unchanged}
print(json.dumps(dict(before=before,restored=restored,after=after,other_shortcuts=unchanged),indent=2))
PY
