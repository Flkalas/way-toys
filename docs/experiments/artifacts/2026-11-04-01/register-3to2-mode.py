#!/usr/bin/python3
"""Register the configured X11 mode and optionally restore it or watch RandR.

The Debian Xsession hook registers before KScreen starts. The session service
confirms the selected mode after login and recovers a missing mode on reconnect.
An available custom mode is left alone when the user selects another resolution.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import time


def log(message):
    print(datetime.datetime.now().astimezone().isoformat(timespec='seconds'), message, flush=True)


def command(*args):
    return subprocess.run(list(map(str,args)),check=True,capture_output=True,text=True,timeout=10).stdout


def find_monitor(query, config):
    for block in re.split(r'(?=^\S+ (?:connected|disconnected)\b)',query,flags=re.M):
        header=re.match(r'^(\S+) connected\b',block)
        edid=re.search(r'^\s+EDID:\s*\n((?:\s+[0-9a-fA-F]{32}\s*\n)+)',block,re.M)
        if not header or not edid:
            continue
        digest=hashlib.sha256(bytes.fromhex(''.join(edid.group(1).split()))).hexdigest()
        if digest==config['edid_sha256']:
            return header.group(1), bool(re.search(r'^\s+'+re.escape(config['mode'])+r'\s',block,re.M))
    return None,False


def register(output, config):
    result=subprocess.run(['xrandr','--newmode',config['mode'],*config['timing']],capture_output=True,text=True,timeout=10)
    if result.returncode and 'BadName' not in result.stderr:
        raise RuntimeError(result.stderr.strip())
    command('xrandr','--addmode',output,config['mode'])
    log(f'{output}: registered {config["mode"]}')


def select_mode(output,config):
    data=json.loads(command('kscreen-doctor','--json'))
    target=next((o for o in data['outputs'] if o['name']==output and o['connected'] and o['enabled']),None)
    if target is None:
        return False
    timing=config['timing']
    width,height=int(timing[1]),int(timing[5])
    refresh=float(timing[0])*1e6/(int(timing[4])*int(timing[8]))
    modes=[m for m in target['modes'] if m['size']=={'width':width,'height':height} and abs(m['refreshRate']-refresh)<.2]
    if not modes:
        return False
    desired=min(modes,key=lambda m:abs(m['refreshRate']-refresh))
    if target['currentModeId']==desired['id']:
        return True
    command('kscreen-doctor',f'output.{output}.mode.{desired["id"]}')
    log(f'{output}: selected {width}x{height}@{desired["refreshRate"]:.5f}')
    return False


def restore(config, wait, apply):
    deadline=time.monotonic()+wait
    stable=0
    while True:
        output,available=find_monitor(command('xrandr','--prop'),config)
        if output:
            if not available:
                register(output,config)
            if not apply:
                log(f'{output}: mode available')
                return True
            stable=stable+1 if select_mode(output,config) else 0
            if stable>=3:
                log(f'{output}: configured mode verified')
                return True
        if time.monotonic()>=deadline:
            if output and apply:
                raise RuntimeError('Configured mode could not be selected before timeout')
            log('Configured monitor is not connected; skipped')
            return False
        time.sleep(.4)


def watch(config,wait):
    from Xlib import display
    from Xlib.ext import randr
    connection=display.Display()
    root=connection.screen().root
    root.xrandr_select_input(randr.RRScreenChangeNotifyMask|randr.RROutputChangeNotifyMask|randr.RROutputPropertyNotifyMask)
    connection.sync()
    output,_=find_monitor(command('xrandr','--prop'),config)
    previous=output
    log('Watching RandR reconnect/mode events')
    try:
        while True:
            # A quiet connection sleeps. No periodic subprocess or key input
            # monitoring is needed; mode/output property events wake this loop.
            if not connection.pending_events():
                select.select([connection],[],[],60)
            if not connection.pending_events():
                continue
            while connection.pending_events():
                connection.next_event()
            time.sleep(.4)
            output,available=find_monitor(command('xrandr','--prop'),config)
            if output and (not available or output!=previous):
                try:
                    restore(config,wait,True)
                except (OSError,RuntimeError,subprocess.SubprocessError) as error:
                    log(f'Recovery failed: {error}')
            previous=output
    finally:
        connection.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config',type=Path,default=Path.home()/'.config/display-modes/3to2.json')
    parser.add_argument('--apply',action='store_true')
    parser.add_argument('--watch',action='store_true')
    parser.add_argument('--wait',type=float,default=0)
    args=parser.parse_args()
    if args.wait<0 or args.wait>45:
        parser.error('--wait must be between 0 and 45 seconds')
    if os.environ.get('XDG_SESSION_TYPE','x11')!='x11' or not os.environ.get('DISPLAY'):
        return
    config=json.loads(args.config.read_text())
    restore(config,args.wait,args.apply or args.watch)
    if args.watch:
        watch(config,max(args.wait,10))


if __name__=='__main__':
    try:
        main()
    except (OSError,ValueError,RuntimeError,subprocess.SubprocessError) as error:
        log(str(error))
        sys.exit(1)
