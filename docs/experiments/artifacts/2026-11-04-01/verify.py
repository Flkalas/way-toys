#!/usr/bin/python3
"""Verify the real Debian hook, login fallback, and live missing-mode recovery.

Temporarily switch only the configured output to its original 3600x2400 mode
while removing this setup's custom mode. Keep the recovery service enabled.
This simulates missing RandR state; it does not log out or reboot the machine.
"""
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import time
from Xlib import display
from Xlib.ext import randr

ART=Path.cwd()/'docs/experiments/artifacts/2026-11-04-01'
USER=Path.home()
CONFIG=json.loads((USER/'.config/display-modes/3to2.json').read_text())
SERVICE='display-3to2.service'
SCRIPT=USER/'.local/bin/register-3to2-mode'
results={}


def run(*args):
    with (ART/'commands.jsonl').open('a') as log:log.write(json.dumps(list(map(str,args)))+'\n')
    return subprocess.check_output(list(map(str,args)),text=True,stderr=subprocess.STDOUT,timeout=40).strip()


def state():
    data=json.loads(run('kscreen-doctor','--json'))
    outputs={}
    for output in data['outputs']:
        if output['connected'] and output['enabled']:
            mode=next(m for m in output['modes'] if m['id']==output['currentModeId'])
            outputs[output['name']]={k:output[k] for k in ('pos','priority','rotation','scale')}
            outputs[output['name']].update(size=mode['size'],refresh=mode['refreshRate'])
    return outputs


def wait(check):
    deadline=time.monotonic()+40
    while time.monotonic()<deadline:
        if check():return
        time.sleep(.2)
    raise RuntimeError('Verification timed out')


def available():
    return CONFIG['mode'] in run('xrandr','--query')


def restored():
    return available() and state()['DP-1-1']['size']=={'width':2544,'height':1696}


def watching():
    pid=run('systemctl','--user','show',SERVICE,'--property=MainPID','--value')
    return 'Watching RandR' in run('journalctl','--user','-u',SERVICE,'_PID='+pid,'--no-pager','-n','20')


def remove_custom_mode():
    # Use the native mode first; RandR cannot destroy an actively used mode.
    run('kscreen-doctor','output.DP-1-1.mode.3600x2400@60')
    connection=display.Display()
    errors=[]
    connection.set_error_handler(lambda error,request:errors.append(str(error)))
    try:
        root=connection.screen().root
        resources=root.xrandr_get_screen_resources_current()
        offset=0
        mode_id=None
        for mode in resources.modes:
            name=resources.names[offset:offset+mode.name_length]
            offset+=mode.name_length
            if name==CONFIG['mode']:mode_id=mode.id
        if mode_id is None:raise RuntimeError('Owned custom mode is already absent')
        output_id=next(output for output in resources.outputs if connection.xrandr_get_output_info(output,resources.config_timestamp).name=='DP-1-1')
        # Delete+destroy atomically so the recovery listener cannot reattach the
        # mode between these two operations. No other mode or output is removed.
        connection.grab_server()
        try:
            connection.xrandr_delete_output_mode(output_id,mode_id)
            connection.xrandr_destroy_mode(mode_id)
            connection.sync()
        finally:
            connection.ungrab_server();connection.sync()
        if errors:raise RuntimeError(str(errors))
    finally:connection.close()


before=state()
results['primary_before']=before['DP-1-2']
try:
    ast.parse(SCRIPT.read_text())
    assert SCRIPT.read_bytes()==(ART/'register-3to2-mode.py').read_bytes()
    for name in ('.xprofile','.xsessionrc'):run('sh','-n',USER/name)
    assert '# BEGIN managed 3:2 display mode' not in (USER/'.xprofile').read_text()
    assert (USER/'.xsessionrc').read_text().count('# BEGIN managed 3:2 display mode')==1
    run('systemctl','--user','stop',SERVICE)
    remove_custom_mode()
    assert not available()
    results['cold_missing_mode']=True
    run('sh','-c','USERXSESSIONRC="$HOME/.xsessionrc"; . /etc/X11/Xsession.d/40x11-common_xsessionrc')
    assert available()
    results['debian_hook_registers']=True
    run('systemctl','--user','start',SERVICE)
    wait(restored);wait(watching)
    results['login_service_selects']=state()
    # Now lose the mode while the listener is running; no service restart or
    # manual registration is performed between the loss and recovery.
    pid=run('systemctl','--user','show',SERVICE,'--property=MainPID','--value')
    remove_custom_mode()
    wait(restored)
    assert pid==run('systemctl','--user','show',SERVICE,'--property=MainPID','--value')
    results['live_recovery_same_pid']=True
    results['live_recovered']=state()
    assert state()['DP-1-2']==before['DP-1-2']
    run('qdbus6','org.kde.kded6','/kded','org.kde.kded6.unloadModule','kscreen')
    assert run('qdbus6','org.kde.kded6','/kded','org.kde.kded6.loadModule','kscreen')=='true'
    time.sleep(1)
    wait(restored)
    results['kscreen_reload_kept_mode']=True
    results['final']=state()
    results['enabled']=run('systemctl','--user','is-enabled',SERVICE)
    results['active']=run('systemctl','--user','is-active',SERVICE)
    results['script_sha256']=hashlib.sha256(SCRIPT.read_bytes()).hexdigest()
    results['relogin_tested']=False
    results['reboot_tested']=False
    results['passed']=True
    print(json.dumps(results))
finally:
    # Always leave the configured service running, including after a failed
    # verification, so its normal recovery can restore the selected mode.
    run('systemctl','--user','start',SERVICE)
    (ART/'verified.json').write_text(json.dumps(results,indent=2)+'\n')
    (ART/'service.log').write_text(run('journalctl','--user','-u',SERVICE,'-b','--no-pager','-n','80')+'\n')
