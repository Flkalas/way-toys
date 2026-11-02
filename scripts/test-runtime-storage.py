#!/usr/bin/env python3
"""Destructive-file fixture test, restricted to the isolated VM test directory."""
import json
import os
from pathlib import Path
import subprocess
import time

base=Path('/home/debian/waytoys-test-results/settings-23')
repo=Path('/home/debian/projects/way-toys')
exe=repo/'target/debug/waytoys'
def run(*args,check=True):
    return subprocess.run([str(a) for a in args],text=True,capture_output=True,check=check).stdout
state=json.loads(run(exe,'--status'))
assert state['paths']=={'app':str(base/'app'),'zones':str(base/'zones'),'paste':str(base/'app/paste')}
path=base/'app/settings.json'
original=path.read_bytes()
run(exe,'--quit');time.sleep(.3)
try:
    path.write_bytes(b'broken-original')
    run('systemd-run','--user','--unit=waytoys-runtime','--collect','--setenv=WAYLAND_DISPLAY=wayland-0','--property=PartOf=graphical-session.target',exe,'--background','--app-config-dir',base/'app','--config-dir',base/'zones')
    time.sleep(.7)
    blocked=json.loads(run(exe,'--status'))
    assert blocked['blocked'] and not blocked['ready'],blocked
    attempt=subprocess.run([str(exe),'--zones','on'],capture_output=True,text=True)
    assert attempt.returncode!=0 and path.read_bytes()==b'broken-original'
finally:
    tmp=path.with_suffix('.fixture-restore');tmp.write_bytes(original);os.replace(tmp,path)
restored=json.loads(run('qdbus6','org.waytoys.Runtime1','/Waytoys','org.waytoys.Runtime1.Reload',''))
assert restored['settings']==json.loads(original) and restored['ready']
print(json.dumps(dict(blocked=blocked,mutation_rejected=True,original_preserved=True,restored=restored),indent=2,ensure_ascii=False))
