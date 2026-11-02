#!/usr/bin/env python3
"""Real VM 700 clipboard tests. Uses only the isolated settings-23 application.

Fixed Qt fixtures produce clipboard data and verify Ctrl+V. QMP sends keyboard,
click and pointer input. Test artifacts never contain arbitrary desktop history.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--output',required=True)
p.add_argument('--only',default='seed')
args=p.parse_args()
out=Path(args.output);out.mkdir(parents=True,exist_ok=True)
repo='/home/debian/projects/way-toys'
base='/home/debian/waytoys-test-results/paste-27'
prefix='export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland\nset -eu\n'
results=[]

def guest(script,binary=False,check=True):
    proc=subprocess.run(['ssh','debian@192.168.50.125','bash -s'],input=(prefix+script+'\n').encode(),capture_output=True,timeout=30)
    if check and proc.returncode:raise RuntimeError((proc.stderr or proc.stdout).decode(errors='replace'))
    if not check:return proc
    return proc.stdout if binary else proc.stdout.decode().strip()

def input_events(name,events):
    (out/(name+'-input.json')).write_text(json.dumps(events,indent=2)+'\n')
    proc=subprocess.run(['ssh','INOPS-PVE','python3 /tmp/waytoys-settings-vm-input.py --vm-id 700 --width 1962 --height 627'],input=json.dumps(events).encode(),capture_output=True,timeout=30)
    if proc.returncode:raise RuntimeError(proc.stderr.decode())

def key(key,down):return dict(type='key',key=key,down=down)
def wait(seconds):return dict(type='wait',seconds=seconds)
def move(x,y):return [dict(type='move',x=x+.5,y=y),dict(type='move',x=x,y=y)]
def click(name,x,y):
    # Cross at a height shared by both outputs; a direct absolute jump through
    # the blank area below the 480px output is clamped to that output's corner.
    bridge=move(x,250) if x>=848 and y>=480 else []
    input_events(name,bridge+move(x,y)+[dict(type='button',button='left',down=True),dict(type='button',button='left',down=False),wait(.4)])
def combo(name,keys):input_events(name,[key(k,True) for k in keys]+[key(k,False) for k in reversed(keys)]+[wait(.5)])
def owner(name):return guest(f'qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner {name}')=='true'
def wait_for(fn,description):
    for _ in range(25):
        value=fn()
        if value:return value
        time.sleep(.15)
    raise AssertionError(description)
def snapshot():return json.loads(guest(f'{repo}/target/debug/waytoys-paste --status'))
def entries():return snapshot()['listing']['entries']
def fixture():
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption WaytoysPasteProbe --place 100 60 602 309')
    time.sleep(.2)
    click('focus-probe',250,180)
def picker():
    if owner('org.waytoys.PastePicker1'):
        combo('close-old-picker',['esc'])
        wait_for(lambda:not owner('org.waytoys.PastePicker1'),'old picker did not close')
    combo('open-picker',['meta_l','v'])
    wait_for(lambda:owner('org.waytoys.PastePicker1'),'Meta+V did not open picker')
    time.sleep(.6)
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard" --place 930 15 580 590')
    time.sleep(.4)
    data=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard"'))
    assert len(data['windows'])==1,data
    return data['windows'][0]
def screenshot(name):
    path=f'{base}/{name}.png'
    guest(f'timeout 10s spectacle --background --fullscreen --nonotify --output {path}')
    (out/(name+'.png')).write_bytes(guest(f'cat {path}',binary=True))

def known_entries():
    values=entries()
    text_entry=next(e for e in values if e['preview']=='Waytoys clipboard 한글 확인\n두 번째 줄 · 27')
    image_entry=next(e for e in values if e['width']==96 and e['height']==64)
    return text_entry,image_entry

def pinned(id):return next(e['pinned'] for e in entries() if e['id']==id)

def paste_result():
    fixture();combo('paste-into-qt',['ctrl','v'])
    return json.loads(guest(f'tail -n 1 {base}/pasted.jsonl'))

def pin_and_paste():
    text_entry,image_entry=known_entries()
    window=picker();x,y,_,_=window['geometry']
    click('image-filter',x+175,y+145)
    if not pinned(image_entry['id']):click('pin-image',x+528,y+196)
    wait_for(lambda:pinned(image_entry['id']),'image pin was not saved')
    click('text-filter',x+101,y+145)
    if not pinned(text_entry['id']):click('pin-text',x+528,y+196)
    wait_for(lambda:pinned(text_entry['id']),'text pin was not saved')
    # Search uses real keystrokes in the focused input.
    click('focus-search',x+130,y+100)
    input_events('search-clipboard',[event for c in 'clipboard' for event in (key(c,True),key(c,False))]+[wait(.5)])
    screenshot('text-search-pinned')
    combo('select-search-result',['ret'])
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'Enter did not select and close')
    pasted_text=paste_result()
    expected='Waytoys clipboard 한글 확인\n두 번째 줄 · 27'.encode()
    assert pasted_text['kind']=='text' and pasted_text['sha256']==hashlib.sha256(expected).hexdigest(),pasted_text
    window=picker();x,y,_,_=window['geometry']
    click('image-filter-copy',x+175,y+145)
    screenshot('image-pinned')
    click('select-image',x+180,y+200)
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'image selection did not close')
    pasted_image=paste_result()
    pixels=bytes(c for yy in range(64) for xx in range(96) for c in (211 if xx<48 else 56,47 if yy<32 else 142,60,255))
    assert pasted_image==dict(kind='image',width=96,height=64,sha256=hashlib.sha256(pixels).hexdigest()),pasted_image
    return dict(text=pasted_text,image=pasted_image,pinned_ids=[text_entry['id'],image_entry['id']])

def preserve_and_restart():
    text_entry,image_entry=known_entries()
    assert pinned(text_entry['id']) and pinned(image_entry['id'])
    # The same method is used by the UI's explicit two-step clear action.
    guest('qdbus6 org.waytoys.Paste1 /Paste org.waytoys.Paste1.ClearRecent')
    state=snapshot();assert state['listing']['recent']==0 and state['listing']['pinned']==2,state
    rejected=guest(f'qdbus6 org.waytoys.Paste1 /Paste org.waytoys.Paste1.Delete {image_entry["id"]}',check=False)
    assert rejected.returncode and pinned(image_entry['id'])
    clipboard=guest('wl-paste --no-newline --type image/png',binary=True)
    guest(f'{repo}/target/debug/waytoys --quit')
    wait_for(lambda:not owner('org.waytoys.Runtime1') and not owner('org.waytoys.Paste1'),'runtime/collector did not stop')
    assert hashlib.sha256(guest('wl-paste --no-newline --type image/png',binary=True)).digest()==hashlib.sha256(clipboard).digest()
    guest(f'systemd-run --user --unit=waytoys-settings-27-restart --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=ICED_BACKEND=tiny-skia {repo}/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page paste')
    wait_for(lambda:owner('org.waytoys.Paste1'),'collector did not restart')
    wait_for(lambda:snapshot()['ready'] and snapshot()['enabled'],'collector did not resume')
    assert pinned(text_entry['id']) and pinned(image_entry['id'])
    window=picker();screenshot('pins-after-restart')
    return dict(pinned_ids=[e['id'] for e in entries() if e['pinned']],clipboard_survived=True,geometry=window)

def seed():
    if not json.loads(guest(f'{repo}/target/debug/waytoys --status'))['settings']['enabled']['paste']:
        # Checkbox observed in settings-before/screen.png, frame 900,15,900,500.
        click('enable-recording',1055,127)
    wait_for(lambda:snapshot()['enabled'],'recording checkbox did not enable history')
    fixture();combo('copy-text',['ctrl','1'])
    text_entry=wait_for(lambda:next((e for e in entries() if e['preview']=='Waytoys clipboard 한글 확인\n두 번째 줄 · 27'),None),'text was not recorded')
    combo('copy-image',['ctrl','2'])
    image_entry=wait_for(lambda:next((e for e in entries() if e['width']==96 and e['height']==64),None),'image was not recorded')
    before=snapshot()['listing']['total']
    combo('sensitive',['ctrl','3']);time.sleep(.6)
    assert snapshot()['listing']['total']==before
    assert not any('DO_NOT_SAVE' in e['preview'] for e in entries())
    geometry=picker();screenshot('picker-seeded')
    return dict(text=text_entry,image=image_entry,sensitive_skipped=True,geometry=geometry)

def pause_and_capture():
    window=picker();x,y,_,_=window['geometry']
    if snapshot()['enabled']:click('pause-recording',x+500,y+56)
    wait_for(lambda:not snapshot()['enabled'],'checkbox did not pause recording')
    combo('close-paused',['esc'])
    before=snapshot()['listing']['total']
    guest("printf 'PAUSED_FIXTURE_27' | wl-copy --type 'text/plain;charset=utf-8' >/dev/null 2>&1")
    time.sleep(.7)
    assert snapshot()['listing']['total']==before
    assert not any('PAUSED_FIXTURE' in e['preview'] for e in entries())
    # Resume with a known public fixture so paused content is never re-imported.
    fixture();combo('copy-public-before-resume',['ctrl','1'])
    window=picker();x,y,_,_=window['geometry']
    click('resume-recording',x+500,y+56)
    wait_for(lambda:snapshot()['enabled'],'checkbox did not resume recording')
    combo('toggle-picker-closed',['meta_l','v'])
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'second Meta+V did not close picker')
    guest("printf 'not a PNG' | wl-copy --type image/png >/dev/null 2>&1")
    wait_for(lambda:snapshot()['error'],'invalid image did not report an error')
    assert not any(e['preview']=='not a PNG' for e in entries())
    fixture();combo('recover-public-text',['ctrl','1'])
    wait_for(lambda:not snapshot()['error'],'valid copy did not clear ingestion error')
    combo('start-waytoys-capture',['meta_l','shift','s'])
    wait_for(lambda:owner('org.waytoys.Capture1'),'Meta+Shift+S did not start capture')
    time.sleep(.7)
    combo('paste-during-capture',['meta_l','v'])
    assert not owner('org.waytoys.PastePicker1'),'picker stole capture focus'
    input_events('capture-region',move(140,120)+[dict(type='button',button='left',down=True),wait(.15)]+move(460,260)+[wait(.2),dict(type='button',button='left',down=False),wait(.7)])
    wait_for(lambda:not owner('org.waytoys.Capture1'),'capture did not finish')
    captured=wait_for(lambda:next((e for e in entries() if 320<=e['width']<=321 and 140<=e['height']<=141),None),'captured image did not appear in history')
    wait_for(lambda:json.loads(guest(f'{repo}/target/debug/waytoys --status'))['ready'],'Zones did not resume')
    picker();screenshot('capture-in-history')
    return dict(paused_skipped=True,invalid_image_rejected=True,capture=captured)


def lock_and_delivery():
    text_entry,image_entry=known_entries()
    picker()
    session=guest('loginctl show-user debian -p Display --value');assert session.isdecimal()
    try:
        guest(f'loginctl lock-session {session}')
        wait_for(lambda:snapshot()['locked'],'collector did not observe screen lock')
        wait_for(lambda:not owner('org.waytoys.PastePicker1'),'picker did not close on lock')
        assert snapshot()['listing']['total']==0
        rejected=guest(f'qdbus6 org.waytoys.Paste1 /Paste org.waytoys.Paste1.Copy {text_entry["id"]}',check=False)
        assert rejected.returncode,'copy accepted while locked'
    finally:
        guest(f'sudo -n loginctl unlock-session {session}')
        input_events('unlock-wake',move(500,250)+[wait(.5)])
    wait_for(lambda:not snapshot()['locked'],'collector did not observe unlock')
    # Known public rows make scrolling/paging repeatable; no arbitrary history dump.
    guest('for number in $(seq 1 14); do\n printf "SCROLL_FIXTURE_27_%02d" "$number" | wl-copy --type text/plain >/dev/null 2>&1\n sleep 0.3\ndone')
    wait_for(lambda:snapshot()['listing']['recent']>=14,'scroll fixtures not collected')
    window=picker();x,y,_,_=window['geometry']
    screenshot('picker-115-percent')
    expected_page=json.loads(guest("qdbus6 org.waytoys.Paste1 /Paste org.waytoys.Paste1.List '' all 1"))['listing']['entries'][0]['preview']
    click('next-page',x+540,y+495)
    screenshot('second-page')
    click('copy-second-page-row',x+180,y+200)
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'page selection did not close')
    page_copy=guest('wl-paste --no-newline --type text/plain')
    assert page_copy==expected_page,(page_copy,expected_page)
    picker()
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard" --place 100 25 560 400')
    time.sleep(.5)
    screenshot('picker-100-percent')
    # Resize the real border with pointer input, not a synthetic UI message.
    input_events('resize-small',move(659,424)+[dict(type='button',button='left',down=True),wait(.15)]+move(545,371)+[wait(.2),dict(type='button',button='left',down=False),wait(.5)])
    small=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard"'))['windows'][0]
    if small['geometry'][2]==560:
        # Breeze's edge hit target can be outside the reported frame. Start
        # KWin's interactive resize operation, then send real arrow-key input.
        guest('qdbus6 org.kde.kglobalaccel /component/kwin org.kde.kglobalaccel.Component.invokeShortcut "Window Resize"')
        time.sleep(.2)
        input_events('resize-keyboard-steps',[e for k in ['left']*12+['up']*5+['ret'] for e in [key(k,True),key(k,False)]]+[wait(.5)])
        small=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard"'))['windows'][0]
    assert 440<=small['geometry'][2]<560,small
    screenshot('picker-small-before-scroll')
    input_events('scroll-small',move(300,225)+[e for _ in range(6) for e in [dict(type='button',button='wheel-down',down=True),dict(type='button',button='wheel-down',down=False)]]+[wait(.4)])
    screenshot('picker-small-scrolled')
    # Move the same live window to the 115% display before measuring it.
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard" --place 930 15 580 590')
    time.sleep(1)
    moved=json.loads(guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption "Waytoys Clipboard"'))['windows'][0]
    (out/'moved-window.json').write_text(json.dumps(moved,indent=2)+'\n')
    screenshot('picker-after-monitor-move')
    return dict(lock_hides_and_refuses_copy=True,small_window=small,page_copy=page_copy,moved_window=moved)


def clear_and_measure():
    text_entry,image_entry=known_entries()
    window=picker();x,y,w,h=window['geometry']
    screenshot('before-clear')
    measure=Path(__file__).resolve().parents[1]/'docs/experiments/artifacts/2026-10-31-27/measure-paste.py'
    subprocess.run(['scp',str(measure),f'debian@192.168.50.125:{base}/measure-paste.py'],check=True,capture_output=True)
    measured=json.loads(guest(f'python3 {base}/measure-paste.py'))
    (out/'memory.json').write_text(json.dumps(measured,indent=2)+'\n')
    click('clear-recent-prompt',x+110,y+h-31)
    screenshot('clear-confirmation')
    click('clear-recent-confirm',x+280,y+h-31)
    wait_for(lambda:snapshot()['listing']['recent']==0,'confirmed clear did not remove recent entries')
    assert pinned(text_entry['id']) and pinned(image_entry['id'])
    screenshot('pinned-final')
    combo('close-after-measuring',['esc'])
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'picker did not close after measurement')
    picker_pid=next(e['pid'] for e in measured['processes'] if e['name']=='org.waytoys.PastePicker1')
    guest(f'test ! -d /proc/{picker_pid}')
    wait_for(lambda:json.loads(guest(f'{repo}/target/debug/waytoys --status'))['ready'],'Zones did not resume after delivery')
    return dict(pins_survived_ui_clear=True,picker_pid_gone=picker_pid)


def final_screen():
    guest('qdbus6 org.waytoys.Settings1 /Settings org.waytoys.Settings1.Activate paste')
    guest(f'python3 {repo}/scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings --place 43 15 762 409')
    time.sleep(.5)
    screenshot('settings-final')
    picker();screenshot('clipboard-final')
    assert snapshot()['listing']['pinned']==2
    combo('final-toggle-close',['meta_l','v'])
    wait_for(lambda:not owner('org.waytoys.PastePicker1'),'installed Meta+V did not toggle close')
    return dict(meta_v_roundtrip=True,pinned_count=2)


tests={'seed':seed,'pin_and_paste':pin_and_paste,'preserve_and_restart':preserve_and_restart,'pause_and_capture':pause_and_capture,'lock_and_delivery':lock_and_delivery,'clear_and_measure':clear_and_measure,'final_screen':final_screen}
try:
    status=json.loads(guest(f'{repo}/target/debug/waytoys --status'))
    assert status['paths']['app']=='/home/debian/waytoys-test-results/settings-23/app',status
    assert status['paths']['paste']=='/home/debian/waytoys-test-results/settings-23/app/paste',status
    subprocess.run(['scp',str(Path(__file__).with_name('vm-input.py')),'INOPS-PVE:/tmp/waytoys-settings-vm-input.py'],check=True,capture_output=True)
    session=guest('loginctl show-user debian -p Display --value');assert session.isdecimal()
    guest(f'sudo -n loginctl unlock-session {session}')
    input_events('wake',move(600,250)+[wait(.4)])
    guest(f'mkdir -p {base}\nsystemctl --user stop waytoys-paste-probe.service 2>/dev/null || true')
    if args.only!='final_screen':guest(f'systemd-run --user --unit=waytoys-paste-probe --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=QT_QPA_PLATFORM=wayland {repo}/target/paste-probe {base}/pasted.jsonl')
    time.sleep(.6)
    for name,fn in tests.items():
        if args.only and name not in args.only.split(','):continue
        try:detail=fn()
        except Exception as e:
            results.append(dict(name=name,passed=False,error=str(e)));raise
        results.append(dict(name=name,passed=True,detail=detail));print(name+': passed',flush=True)
        (out/'results.json').write_text(json.dumps(results,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
finally:
    (out/'results.json').write_text(json.dumps(results,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
    guest('qdbus6 org.waytoys.PastePicker1 /Picker org.waytoys.PastePicker1.Close 2>/dev/null || true\nsystemctl --user stop waytoys-paste-probe.service 2>/dev/null || true',check=False)
