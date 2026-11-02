#!/usr/bin/env python3
"""Read native KWin window geometry/opacity, optionally prepare one test window.

Uses the official scripting D-Bus API. No permanent KWin settings are changed.
"""
import argparse
import json
import os
import tempfile
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

parser = argparse.ArgumentParser()
parser.add_argument('--caption', default='WaytoysProbe')
parser.add_argument('--app')
parser.add_argument('--place', nargs=4, type=int, metavar=('X', 'Y', 'W', 'H'))
parser.add_argument('--opacity', type=float)
args = parser.parse_args()
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName('org.waytoys.TestProbe', bus=bus, do_not_queue=True)
loop = GLib.MainLoop()
result = []

class Receiver(dbus.service.Object):
    @dbus.service.method('org.waytoys.TestProbe', in_signature='s')
    def Report(self, value):
        result.append(json.loads(value))
        loop.quit()

receiver = Receiver(bus, '/Probe')
prepare = ''
if args.place:
    x, y, w, h = args.place
    prepare += f'w.frameGeometry = {{x:{x},y:{y},width:{w},height:{h}}}; workspace.activeWindow = w;'
if args.opacity is not None:
    prepare += f'w.opacity = {args.opacity};'
script = '''let report=[];
for (const w of workspace.windowList()) {
 if (w.caption.indexOf(CAPTION) < 0) continue;
 if (APP !== null && w.resourceClass !== APP) continue;
 PREPARE
 let r=w.frameGeometry;
 report.push({id:String(w.internalId),caption:w.caption,app:w.resourceClass,geometry:[r.x,r.y,r.width,r.height],opacity:w.opacity,move:w.move,resize:w.resize,minimized:w.minimized});
}
callDBus('org.waytoys.TestProbe','/Probe','org.waytoys.TestProbe','Report',JSON.stringify({pointer:workspace.cursorPos,windows:report}));
'''.replace('CAPTION', json.dumps(args.caption)).replace('APP', json.dumps(args.app)).replace('PREPARE', prepare)
fd, path = tempfile.mkstemp(prefix='waytoys-window-', suffix='.js')
with os.fdopen(fd, 'w') as f:
    f.write(script)
scripting = dbus.Interface(bus.get_object('org.kde.KWin', '/Scripting'), 'org.kde.kwin.Scripting')
script_name = f'waytoys-test-probe-{os.getpid()}'
try:
    sid = bus.call_blocking('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'loadScript', 'ss', (path, script_name))
    obj = dbus.Interface(bus.get_object('org.kde.KWin', f'/Scripting/Script{sid}'), 'org.kde.kwin.Script')
    obj.run()
    GLib.timeout_add(3000, lambda: loop.quit())
    loop.run()
    if not result:
        raise RuntimeError('KWin probe response timed out')
    print(json.dumps(result[0], ensure_ascii=False))
finally:
    scripting.unloadScript(script_name)
    os.unlink(path)
