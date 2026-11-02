#!/usr/bin/env python3
"""Exercise Waytoys' actual StatusNotifierItem/DBusMenu endpoints in the VM."""
import argparse
import json
import subprocess
import dbus

p=argparse.ArgumentParser()
p.add_argument('action',choices=['open','quit'])
a=p.parse_args()
pid=int(subprocess.check_output(['systemctl','--user','show','waytoys-runtime.service','-p','MainPID','--value']))
bus=dbus.SessionBus()
props=dbus.Interface(bus.get_object('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher'),'org.freedesktop.DBus.Properties')
items=props.Get('org.kde.StatusNotifierWatcher','RegisteredStatusNotifierItems')
for item in items:
    service,_,path=str(item).partition('/')
    owner_pid=bus.call_blocking('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','GetConnectionUnixProcessID','s',(service,))
    if owner_pid!=pid: continue
    path='/'+path if path else '/StatusNotifierItem'
    obj=bus.get_object(service,path)
    values=dbus.Interface(obj,'org.freedesktop.DBus.Properties').GetAll('org.kde.StatusNotifierItem')
    if str(values['Id'])!='waytoys': continue
    if a.action=='open':
        dbus.Interface(obj,'org.kde.StatusNotifierItem').Activate(0,0)
        print(json.dumps(dict(action='open',pid=pid,item=str(item))))
    else:
        menu=dbus.Interface(bus.get_object(service,values['Menu']),'com.canonical.dbusmenu')
        _,layout=menu.GetLayout(0,-1,dbus.Array([],signature='s'))
        def children(node):
            yield node
            for child in node[2]: yield from children(child)
        node=next(node for node in children(layout) if str(node[1].get('label',''))=='Waytoys 종료')
        menu.Event(node[0],'clicked',dbus.Int32(0,variant_level=1),dbus.UInt32(0))
        print(json.dumps(dict(action='quit',pid=pid,menu_id=int(node[0]))))
    break
else: raise RuntimeError('No tray item owned by the Waytoys runtime')
