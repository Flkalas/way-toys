#!/usr/bin/env python3
"""VM-only fixed-rectangle adapter probe; never reads/writes layout files."""
import json
import signal
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
owner = dbus.service.BusName('org.waytoys.Runtime1', bus=bus, do_not_queue=True)
bridge = dbus.Interface(bus.get_object('org.kde.KWin', '/Waytoys/Zones'), 'org.waytoys.ZonesBridge1')
snapshot = json.loads(bridge.Snapshot())
outputs = {o['name']: o['geometry'] for o in snapshot['outputs']}
config = json.dumps(dict(protocol=1, enabled=True, outputs=list(outputs), excluded=[], transparent=True))

def on_event(value):
    event = json.loads(value)
    print(json.dumps(event), flush=True)
    if event['kind'] == 'drop':
        x, y, w, h = outputs[event['output']]
        result = dict(epoch=event.get('epoch', ''), id=event['id'], sequence=event['sequence'], rect=[x + 16, y + 16, min(400, w - 32), h - 32])
        bridge.Target(json.dumps(result), reply_handler=lambda ok: print('target accepted:', ok, flush=True), error_handler=lambda error: print(error, flush=True))

bus.add_signal_receiver(on_event, signal_name='Event', dbus_interface='org.waytoys.ZonesBridge1', path='/Waytoys/Zones')
assert bridge.Configure(config)
def heartbeat():
    bridge.Configure(config)
    return True
GLib.timeout_add(1000, heartbeat)
loop = GLib.MainLoop()
def stop(*_):
    bridge.Configure(json.dumps(dict(protocol=1, enabled=False)))
    loop.quit()
signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
print('probe ready', outputs, flush=True)
loop.run()
