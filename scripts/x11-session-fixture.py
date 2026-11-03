#!/usr/bin/env python3
"""Screen-lock service fixture for the isolated X11 integration session only."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName('org.freedesktop.ScreenSaver', bus=bus)

class Saver(dbus.service.Object):
    active = False

    @dbus.service.method('org.freedesktop.ScreenSaver', out_signature='b')
    def GetActive(self):
        return self.active

    @dbus.service.method('org.freedesktop.ScreenSaver', in_signature='b')
    def SetActive(self, active):
        self.active = bool(active)
        self.ActiveChanged(self.active)

    @dbus.service.signal('org.freedesktop.ScreenSaver', signature='b')
    def ActiveChanged(self, active):
        pass

saver = Saver(bus, '/ScreenSaver')
GLib.MainLoop().run()
