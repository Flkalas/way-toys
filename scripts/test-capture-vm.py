#!/usr/bin/env python3
"""Real VM 700 QMP keyboard/mouse capture tests; run with Windows python3.13.

Requires the installed Capture shortcut and the existing isolated settings-23
runtime. Only the explicitly named test fixture is opened/closed. PNG artifacts
are read from the guest clipboard for verification; the product does not save them.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import time
import zlib

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--guest',default='debian@192.168.50.125')
parser.add_argument('--pve',default='INOPS-PVE')
parser.add_argument('--output',required=True)
parser.add_argument('--only',default='')
args=parser.parse_args()
out=Path(args.output);out.mkdir(parents=True,exist_ok=True)
repo='/home/debian/projects/way-toys'
fixture='/home/debian/waytoys-test-results/capture-26'
prefix='export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland\nset -eu\n'
results=[]

def guest(script,binary=False,check=True):
    p=subprocess.run(['ssh',args.guest,'bash -s'],input=(prefix+script+'\n').encode(),capture_output=True,timeout=25)
    if check and p.returncode: raise RuntimeError((p.stderr or p.stdout).decode(errors='replace'))
    if not check:return p
    return p.stdout if binary else p.stdout.decode().strip()

def event_key(key,down):return dict(type='key',key=key,down=down)
def move(x,y):return [dict(type='move',x=x+.5,y=y),dict(type='move',x=x,y=y)]
def button(down,which='left'):return dict(type='button',button=which,down=down)
def wait(seconds):return dict(type='wait',seconds=seconds)
def input_events(name,events):
    (out/(name+'-input.json')).write_text(json.dumps(events,indent=2)+'\n')
    p=subprocess.run(['ssh',args.pve,'python3 /tmp/waytoys-settings-vm-input.py --vm-id 700 --width 1962 --height 627'],input=json.dumps(events).encode(),capture_output=True,timeout=25)
    if p.returncode:raise RuntimeError(p.stderr.decode())
def present():return guest('qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Capture1')=='true'
def await_capture(value):
    for _ in range(20):
        if present()==value:return
        time.sleep(.15)
    raise AssertionError(f'Capture owner did not become {value}')
def start(name):
    assert not present(),'A capture is already active'
    input_events(name+'-hotkey',[event_key('meta_l',True),event_key('shift',True),event_key('s',True),event_key('s',False),event_key('shift',False),event_key('meta_l',False),wait(.7)])
    await_capture(True)
    time.sleep(.3)
def drag(name,a,b):
    start(name)
    input_events(name,move(*a)+[button(True),wait(.1)]+move(*b)+[wait(.25),button(False),wait(.4)])
    await_capture(False)
def clipboard():return guest('wl-paste --no-newline --type image/png',binary=True)
def digest():return hashlib.sha256(clipboard()).hexdigest()

def decode(png):
    assert png[:8]==b'\x89PNG\r\n\x1a\n'
    pos=8;data=b'';w=h=None
    while pos<len(png):
        n=struct.unpack('>I',png[pos:pos+4])[0];kind=png[pos+4:pos+8];chunk=png[pos+8:pos+8+n];pos+=12+n
        if kind==b'IHDR':
            w,h,depth,color,_,_,interlace=struct.unpack('>IIBBBBB',chunk)
            assert depth==8 and color in (2,6) and interlace==0,(depth,color,interlace)
        elif kind==b'IDAT':data+=chunk
    channels=4 if color==6 else 3
    raw=zlib.decompress(data);stride=w*channels;previous=bytearray(stride);rgba=bytearray()
    for y in range(h):
        kind=raw[y*(stride+1)];row=bytearray(raw[y*(stride+1)+1:(y+1)*(stride+1)])
        for i in range(stride):
            a=row[i-channels] if i>=channels else 0;b=previous[i];c=previous[i-channels] if i>=channels else 0
            if kind==1:prediction=a
            elif kind==2:prediction=b
            elif kind==3:prediction=(a+b)//2
            elif kind==4:
                p=a+b-c;pa,pb,pc=abs(p-a),abs(p-b),abs(p-c)
                prediction=a if pa<=pb and pa<=pc else b if pb<=pc else c
            else:assert kind==0;prediction=0
            row[i]=(row[i]+prediction)&255
        if channels==4:rgba.extend(row)
        else:
            for x in range(0,len(row),3):rgba.extend(row[x:x+3]+b'\xff')
        previous=row
    return w,h,bytes(rgba)

def observe():
    data=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption WaytoysCaptureProbe'))
    assert len(data['windows'])==1,data
    return data['windows'][0]
def place(x=100,y=60):
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption WaytoysCaptureProbe --place {x} {y} 502 349')
    time.sleep(.3)
    return observe()
def save_image(name):
    image=clipboard();(out/(name+'.png')).write_bytes(image)
    w,h,rgba=decode(image)
    return dict(width=w,height=h,sha256=hashlib.sha256(image).hexdigest(),rgba_sha256=hashlib.sha256(rgba).hexdigest()),rgba

def single_and_paste():
    place();input_events('reset',[event_key('r',True),event_key('r',False)])
    drag('single',(140,120),(460,260))
    info,rgba=save_image('single')
    # QMP's 16-bit absolute tablet coordinates can land just below an integer
    # logical pixel. Outward rounding may include one additional edge pixel.
    assert 320<=info['width']<=321 and 140<=info['height']<=141,info
    palette={rgba[i:i+4].hex() for i in range(0,len(rgba),4)}
    assert palette=={'d32f2fff','388e3cff','1976d2ff','fbc02dff'},palette
    input_events('paste',[event_key('ctrl',True),event_key('v',True),event_key('v',False),event_key('ctrl',False),wait(.5)])
    rows=guest('journalctl --user -u waytoys-capture-probe.service --no-pager -o cat').splitlines()
    pasted=json.loads(next(line for line in reversed(rows) if line.startswith('{"paste":')))
    assert pasted['paste'] and pasted['saved'] and pasted['rgba_sha256']==info['rgba_sha256'],pasted
    return dict(image=info,pasted=pasted,palette=sorted(palette))

def fractional():
    place(900,60);input_events('reset-fractional',[event_key('r',True),event_key('r',False)])
    drag('fractional',(950,140),(1250,340))
    first,_=save_image('fractional')
    expected=(math.ceil(402*1280/1113)-math.floor(102*1280/1113),math.ceil(340*720/626)-math.floor(140*720/626))
    assert all(abs(got-want)<=2 for got,want in zip((first['width'],first['height']),expected)),(first,expected)
    drag('reverse',(1250,340),(950,140))
    reverse,_=save_image('reverse')
    assert first['rgba_sha256']==reverse['rgba_sha256'],(first,reverse)
    return dict(forward=first,reverse=reverse)

def crossing():
    drag('cross',(700,150),(1050,300))
    info,_=save_image('cross')
    expected=(math.ceil(350*720/626),math.ceil(150*720/626))
    assert all(abs(got-want)<=2 for got,want in zip((info['width'],info['height']),expected)),info
    return info

def cancellation():
    original=digest()
    start('escape')
    input_events('escape',move(300,180)+[button(True)]+move(500,300)+[event_key('esc',True),event_key('esc',False),button(False),wait(.3)])
    await_capture(False);assert digest()==original
    start('right-click');input_events('right-click',[button(True,'right'),button(False,'right'),wait(.3)])
    await_capture(False);assert digest()==original
    start('zero-area');input_events('zero-area',move(250,200)+[button(True),button(False),wait(.3)])
    assert present() and digest()==original
    duplicate=guest(f'{repo}/target/debug/waytoys-capture')
    assert duplicate=='capture already running',duplicate
    guest('qdbus6 org.waytoys.Capture1 /Capture org.waytoys.Capture1.Cancel')
    await_capture(False);assert digest()==original
    return dict(preserved_sha256=original,duplicate=duplicate)

def cleanup_and_resume():
    original=digest();start('terminate')
    guest("pid=$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.GetConnectionUnixProcessID org.waytoys.Capture1); kill -TERM \"$pid\"")
    await_capture(False);assert digest()==original
    time.sleep(.7)
    status=json.loads(guest(f'{repo}/target/debug/waytoys --status'))
    assert status['ready'],status
    place()
    input_events('zones-resume',move(300,74)+[button(True)]+move(330,100)+[event_key('shift',True)]+move(700,200)+[wait(.3),button(False),event_key('shift',False),wait(.5)])
    window=observe()
    assert all(abs(a-b)<1.1 for a,b in zip(window['geometry'],[573,16,259,448])),window
    assert digest()==original
    return dict(status=status,window=window,clipboard_sha256=original)

def failures_and_lifetime():
    original=digest()
    original_pixels=decode(clipboard())
    denied=f'{fixture}/unregistered-capture'
    guest(f'cp {repo}/target/debug/waytoys-capture {denied}')
    try:
        result=guest(f'{denied} --check',check=False)
        log=(result.stdout+result.stderr).decode(errors='replace')
        (out/'denied.log').write_text(log,encoding='utf-8')
        assert result.returncode and 'NoAuthorized' in log,log
    finally:guest(f'rm -f {denied}')
    assert digest()==original
    session=guest('loginctl show-user debian -p Display --value')
    assert session.isdecimal(),session
    guest(f'sudo -n loginctl lock-session {session}')
    try:
        time.sleep(.5)
        assert guest(f'loginctl show-session {session} -p LockedHint --value')=='yes'
        result=guest(f'{repo}/target/debug/waytoys-capture --check',check=False)
        log=(result.stdout+result.stderr).decode(errors='replace')
        (out/'locked.log').write_text(log,encoding='utf-8')
        assert result.returncode,log
    finally:
        guest(f'sudo -n loginctl unlock-session {session}')
        input_events('unlock-wake',move(600,250)+[wait(.5)])
        for _ in range(20):
            if guest('qdbus6 org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver.GetActive')=='false':break
            time.sleep(.2)
        else:raise AssertionError('Screen remained locked')
    # KDE/Klipper can take ownership and re-encode PNG while locking; compare
    # decoded image pixels, not a particular PNG compression stream.
    assert decode(clipboard())==original_pixels,'Image content changed across lock/unlock'
    place();drag('after-lock',(140,120),(460,260))
    original=digest()
    (out/'clipboard-scope.log').write_text(guest('systemctl --user list-units "waytoys-clipboard*" --no-legend\npgrep -a wl-copy')+'\n')
    guest(f'{repo}/target/debug/waytoys --quit')
    for _ in range(15):
        if guest('qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Runtime1')=='false':break
        time.sleep(.2)
    else:raise AssertionError('Runtime did not stop')
    assert digest()==original,'Clipboard owner did not survive runtime exit'
    guest(f'{repo}/target/debug/waytoys-capture --check')
    start('standalone')
    input_events('standalone-cancel',[event_key('esc',True),event_key('esc',False),wait(.3)])
    await_capture(False);assert digest()==original
    guest(f'systemd-run --user --unit=waytoys-settings-26-final --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=ICED_BACKEND=tiny-skia {repo}/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page capture')
    time.sleep(1)
    status=json.loads(guest(f'{repo}/target/debug/waytoys --status'))
    assert status['ready'],status
    return dict(clipboard_sha256=original,status=status,permission_denied=True,locked_refused=True,standalone=True)

def settings_button():
    def settings():
        return json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings'))['windows'][0]
    activate=f'{repo}/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page capture'
    guest(activate)
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings --place 900 15 900 500')
    time.sleep(.5)
    before=settings();original=digest()
    x,y,_,_=before['geometry']
    # Coordinates observed in settings/screen.png at this fixed window size.
    input_events('settings-button',move(x+211,y+198)+[button(True),button(False),wait(.7)])
    await_capture(True)
    assert guest('qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Settings1')=='false','Settings did not close'
    input_events('settings-cancel',[event_key('esc',True),event_key('esc',False),wait(.3)])
    await_capture(False);assert digest()==original
    tray=guest(f'python3 {repo}/scripts/test-tray-action.py open')
    time.sleep(.8)
    restored=settings();assert not restored['minimized'],restored
    assert restored['id']!=before['id'],restored
    guest(activate)
    return dict(before=before,closed=True,restored=restored,tray=json.loads(tray),clipboard_sha256=original)


def preview_and_memory():
    place();input_events('reset-preview',[event_key('r',True),event_key('r',False)])
    start('selection-preview')
    pid=int(guest('qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.GetConnectionUnixProcessID org.waytoys.Capture1'))
    events=move(140,120)+[button(True)]+move(460,260)+[wait(10),button(False),wait(.4)]
    (out/'selection-preview-input.json').write_text(json.dumps(events,indent=2)+'\n')
    pending=subprocess.Popen(['ssh',args.pve,'python3 /tmp/waytoys-settings-vm-input.py --vm-id 700 --width 1962 --height 627'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    pending.stdin.write(json.dumps(events).encode());pending.stdin.close();pending.stdin=None
    try:
        time.sleep(.8)
        guest(f'timeout 8s spectacle --background --fullscreen --nonotify --output {fixture}/selection-screen.png')
        (out/'selection-screen.png').write_bytes(guest(f'cat {fixture}/selection-screen.png',binary=True))
        measure='''import datetime,json,os,time
from pathlib import Path
root=Path('/proc')/str(PID)
def sample():
    status=dict(line.split(':',1) for line in (root/'status').read_text().splitlines())
    memory=dict(line.split(':',1) for line in (root/'smaps_rollup').read_text().splitlines()[1:])
    stat=(root/'stat').read_text().rsplit(') ',1)[1].split()
    return dict(rss_kib=int(status['VmRSS'].split()[0]),peak_rss_kib=int(status['VmHWM'].split()[0]),pss_kib=int(memory['Pss'].split()[0]),threads=int(status['Threads']),cpu_ticks=int(stat[11])+int(stat[12]))
before=sample();start=time.monotonic();time.sleep(3);after=sample();elapsed=time.monotonic()-start
print(json.dumps(dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),pid=PID,elapsed_seconds=elapsed,before=before,after=after,cpu_seconds=(after['cpu_ticks']-before['cpu_ticks'])/os.sysconf('SC_CLK_TCK'))))
'''.replace('PID',str(pid))
        memory=json.loads(guest("python3 - <<'PY'\n"+measure+'PY'))
        (out/'memory.json').write_text(json.dumps(memory,indent=2)+'\n')
    finally:
        stdout,stderr=pending.communicate(timeout=20)
        assert pending.returncode==0,(stdout,stderr)
    await_capture(False)
    image,_=save_image('selected-region')
    assert guest(f'if test -d /proc/{pid}; then echo alive; else echo exited; fi')=='exited'
    cleanup=guest('systemctl --user list-units "waytoys-clipboard*" --no-legend\npgrep -a wl-copy')
    (out/'cleanup.log').write_text(cleanup+'\n')
    return dict(image=image,memory=memory,capture_exited=True)


tests={'single_and_paste':single_and_paste,'fractional':fractional,'crossing':crossing,'cancellation':cancellation,'cleanup_and_resume':cleanup_and_resume,'failures_and_lifetime':failures_and_lifetime,'settings_button':settings_button,'preview_and_memory':preview_and_memory}
try:
    status=json.loads(guest(f'{repo}/target/debug/waytoys --status'))
    assert status['paths']['app']=='/home/debian/waytoys-test-results/settings-23/app',status
    subprocess.run(['scp',str(Path(__file__).with_name('vm-input.py')),args.pve+':/tmp/waytoys-settings-vm-input.py'],check=True,capture_output=True)
    guest(f'mkdir -p {fixture}; systemctl --user stop waytoys-capture-probe.service 2>/dev/null || true')
    guest(f'systemd-run --user --unit=waytoys-capture-probe --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=QT_QPA_PLATFORM=wayland {repo}/target/capture-paste-probe {fixture}/pasted.png')
    time.sleep(.7)
    for name,fn in tests.items():
        if args.only and name not in args.only.split(','):continue
        try:detail=fn()
        except Exception as e:
            results.append(dict(name=name,passed=False,error=str(e)));raise
        results.append(dict(name=name,passed=True,detail=detail))
        (out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
        print(name+': passed',flush=True)
finally:
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
    guest('qdbus6 org.waytoys.Capture1 /Capture org.waytoys.Capture1.Cancel 2>/dev/null || true\nsystemctl --user stop waytoys-capture-probe.service 2>/dev/null || true',check=False)
