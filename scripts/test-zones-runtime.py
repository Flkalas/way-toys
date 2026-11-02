#!/usr/bin/env python3
"""VM 700 integration checks using real QMP input and native KWin observations.

Requires the round-23 isolated fixture and running waytoys-runtime-23 / probe-window
units. Run from the Windows checkout with python3.13. Does not edit layout files.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--guest', default='debian@192.168.50.125')
p.add_argument('--pve', default='INOPS-PVE')
p.add_argument('--output', required=True)
p.add_argument('--only', default='')
p.add_argument('--unit', default='waytoys-runtime-23')
a = p.parse_args()
out = Path(a.output)
out.mkdir(parents=True, exist_ok=True)
repo = '/home/debian/projects/way-toys'
config = '/home/debian/waytoys-test-results/settings-23'
prefix = 'export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland\n'
results = []

def guest(script):
    r = subprocess.run(['ssh', a.guest, 'bash -s'], input=(prefix + 'set -eu\n' + script + '\n').encode(), capture_output=True, timeout=20)
    if r.returncode:
        raise RuntimeError((r.stderr or r.stdout).decode('utf-8'))
    return r.stdout.decode('utf-8').strip()

def status():
    return json.loads(guest(f'{repo}/target/debug/waytoys --status'))

def observe():
    v = json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py'))
    if len(v['windows']) != 1:
        raise AssertionError(v)
    return v['windows'][0]

def place():
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --place 100 60 500 320 --opacity 0.8')
    time.sleep(.25)

def move(x, y):
    # KWin's screen-edge barrier can clamp the first absolute event. QEMU drops
    # identical tablet coordinates, so send a distinct second coordinate.
    return [dict(type='move', x=x+.5, y=y), dict(type='move', x=x, y=y)]

def key(k, down): return dict(type='key', key=k, down=down)
def button(down): return dict(type='button', down=down)
def wait(s): return dict(type='wait', seconds=s)

def start_drag(mid=False):
    e = move(300, 74)
    if not mid: e += [key('shift', True)]
    e += [button(True)] + move(330, 100)
    if mid: e += [key('shift', True)]
    return e

def finish(): return [button(False), key('shift', False), key('ctrl', False), wait(.6)]

def input_events(name, events, callback=None, delay=1.5):
    (out / (name + '-input.json')).write_text(json.dumps(events, indent=2) + '\n')
    proc = subprocess.Popen(['ssh', a.pve, 'python3 /tmp/waytoys-settings-vm-input.py --vm-id 700 --width 1962 --height 627'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    proc.stdin.write(json.dumps(events)); proc.stdin.close(); proc.stdin = None
    try:
        if callback:
            time.sleep(delay)
            callback()
        stdout, stderr = proc.communicate(timeout=45)
        if proc.returncode: raise RuntimeError(stderr)
    finally:
        if proc.poll() is None: proc.wait(timeout=45)

def pref(**changes):
    data = json.dumps(changes)
    guest(f'''python3 - <<'PY'
import json, os
from pathlib import Path
p=Path('{config}/zones/preferences.json')
v=json.loads(p.read_text()); v.update(json.loads({data!r}))
t=p.with_suffix('.test-tmp'); t.write_text(json.dumps(v)); os.replace(t,p)
PY''')
    time.sleep(.8)

def check_geometry(w, expected, tolerance=1.1):
    assert all(abs(x-y) <= tolerance for x,y in zip(w['geometry'], expected)), (w, expected)
    assert abs(w['opacity']-.8)<.001 and not w['move'], w

def test(name, fn):
    if a.only and name not in a.only.split(','): return
    print('RUN', name, flush=True)
    place()
    data = fn()
    results.append(dict(test=name, passed=True, observed=data))
    (out/'results.json').write_text(json.dumps(results, indent=2, ensure_ascii=False)+'\n', encoding='utf-8')
    print('PASS', name, json.dumps(data), flush=True)

def single():
    e=start_drag(True)+move(650,230)+[wait(1),dict(type='capture',path='/tmp/waytoys-runtime-drag.ppm')]+finish()
    input_events('mid-shift',e)
    w=observe();check_geometry(w,[573,16,259,448]);return w

def multi():
    input_events('ctrl-span',start_drag()+move(200,230)+[key('ctrl',True)]+move(650,230)+[wait(.4)]+finish())
    w=observe();check_geometry(w,[16,16,816,448]);return w

def crossing():
    input_events('cross-output',start_drag()+move(200,230)+[key('ctrl',True)]+move(650,230)+move(1000,230)+[wait(.4)]+finish())
    w=observe();x,y,width,height=w['geometry'];assert x>=848 and x+width<=1961 and width<300 and height>590,w
    assert abs(w['opacity']-.8)<.001;return w

def release():
    during=[]
    def sample():
        w=observe();assert w['move'] and abs(w['opacity']-.8)<.001,w;during.append(w)
    input_events('shift-release',start_drag()+move(650,230)+[key('shift',False),wait(5)]+finish(),sample)
    w=observe();assert abs(w['geometry'][2]-500)<1 and abs(w['opacity']-.8)<.001,w
    return dict(during=during,after=w)

def escape():
    input_events('escape',start_drag()+move(650,230)+[key('esc',True),key('esc',False),wait(.4)]+finish())
    w=observe();check_geometry(w,[100,60,500,320]);return w

def disable():
    data=[]
    def off():
        w=observe();assert abs(w['opacity']-.4)<.001,w;data.append(w)
        guest(f'{repo}/target/debug/waytoys --zones off');data.append(status())
        w=observe();assert abs(w['opacity']-.8)<.001,w;data.append(w)
    input_events('disable-owned',start_drag()+move(650,230)+[wait(7),key('shift',False)]+finish(),off)
    guest(f'{repo}/target/debug/waytoys --zones on');assert status()['ready'];return data

def frozen():
    data=[]
    def freeze():
        guest(f'systemctl --user kill --signal=SIGSTOP {shlex.quote(a.unit)}')
        try:
            time.sleep(4.5);w=observe();assert abs(w['opacity']-.8)<.001,w;data.append(w)
        finally: guest(f'systemctl --user kill --signal=SIGCONT {shlex.quote(a.unit)}')
    input_events('lease-timeout',start_drag()+move(650,230)+[wait(10),wait(2)]+finish(),freeze)
    w=observe();check_geometry(w,[638,4,206,432]);data.append(w);return data

def killed():
    data=[]
    def kill():
        guest(f'systemctl --user kill --signal=SIGKILL {shlex.quote(a.unit)}')
        time.sleep(.4);w=observe();assert abs(w['opacity']-.8)<.001,w;data.append(w)
    input_events('host-killed',start_drag()+move(650,230)+[wait(5)]+finish(),kill)
    w=observe();check_geometry(w,[638,4,206,432]);data.append(w)
    if a.unit != 'waytoys.service':
        guest(f'systemd-run --user --unit={shlex.quote(a.unit)} --collect --setenv=WAYLAND_DISPLAY=wayland-0 {repo}/target/debug/waytoys --background --app-config-dir {config}/app --config-dir {config}/zones')
    time.sleep(1);assert status()['ready'];return data

def excluded():
    pref(excluded_apps='org.kde.konsole')
    input_events('excluded',start_drag()+move(650,230)+[wait(.3)]+finish())
    w=observe();check_geometry(w,[638,4,206,432]);pref(excluded_apps='ORG.KDE.KONSOLE')
    place();input_events('case-sensitive',start_drag()+move(650,230)+[wait(.3)]+finish())
    case=observe();check_geometry(case,[573,16,259,448]);pref(excluded_apps='');return dict(excluded=w,different_case=case)

def corrupt():
    path=f'{config}/zones/preferences.json'
    old=guest(f'cat {path}')
    try:
        guest(f"printf '%s' 'broken-original' > {path}");time.sleep(1)
        s=status();assert not s['ready'] and s['error'],s
        assert guest(f'cat {path}')=='broken-original'
        return s
    finally:
        guest(f"printf '%s' {shlex.quote(old)} > {path}");time.sleep(1)
        assert status()['ready']

def screen_lock():
    data=[]
    def lock():
        guest('qdbus6 org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver.Lock');time.sleep(1)
        w=observe();assert abs(w['opacity']-.8)<.001,w;data.append(w)
    try: input_events('screen-lock',start_drag()+move(650,230)+[wait(5)]+finish(),lock)
    finally: guest('sudo -n loginctl unlock-session "$(loginctl show-user 1000 -p Display --value)"')
    return data

def restart_probe():
    guest('systemd-run --user --unit=waytoys-probe-window --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=QT_QPA_PLATFORM=wayland /usr/bin/konsole --separate -p tabtitle=WaytoysProbe -e bash --norc')
    time.sleep(.7)

def closed_window():
    def close(): guest('systemctl --user stop waytoys-probe-window')
    input_events('closed-window',start_drag()+move(650,230)+[wait(5)]+finish(),close)
    w=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py'))['windows'];assert not w,w
    assert status()['ready'];restart_probe();return dict(windows=w,ready=True)

def x11():
    guest('systemctl --user stop waytoys-probe-window')
    guest('systemd-run --user --unit=waytoys-x11-probe --collect /usr/bin/xterm -title WaytoysProbe -e bash --norc')
    try:
        time.sleep(.7);place()
        input_events('x11',start_drag()+move(650,230)+[wait(.5)]+finish())
        w=observe();x,y,width,height=w['geometry']
        assert abs(x-573)<1 and abs(y-16)<1 and 200<width<=260 and 400<height<=449,w
        assert abs(w['opacity']-.8)<.001;return w
    finally:
        guest('systemctl --user stop waytoys-x11-probe');restart_probe()

def size_constraint():
    guest('systemctl --user stop waytoys-probe-window')
    guest(f'systemd-run --user --unit=waytoys-constraint-probe --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=QT_QPA_PLATFORM=wayland {repo}/target/constraint-window')
    try:
        time.sleep(.7)
        guest(f'python3 {repo}/scripts/kwin-window-probe.py --place 100 60 620 550 --opacity 0.8')
        time.sleep(.3)
        before=observe()
        input_events('size-constraint',start_drag()+move(650,230)+[wait(.5)]+finish())
        w=observe();assert w['geometry'][2]>=600 and w['geometry'][3]>=500 and abs(w['opacity']-.8)<.001,w
        return dict(before=before,after=w)
    finally:
        guest('systemctl --user stop waytoys-constraint-probe');restart_probe()

before=guest(f'sha256sum {config}/zones/outputs/*.json')
assert status()['ready'],status()
for name,fn in [('mid_shift',single),('ctrl_span',multi),('cross_output',crossing),('shift_release',release),('escape',escape),('disable',disable),('lease_timeout',frozen),('host_killed',killed),('excluded_case',excluded),('corrupt_preferences',corrupt),('screen_lock',screen_lock),('window_closed',closed_window),('x11',x11),('size_constraint',size_constraint)]:
    test(name,fn)
assert guest(f'sha256sum {config}/zones/outputs/*.json')==before
(out/'layout-hashes.txt').write_text(before+'\n')
print('PASS layout files unchanged',flush=True)
