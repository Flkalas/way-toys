#!/usr/bin/python3
"""Verify the installed login action and a warm reload of KScreen only."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import time

artifact = Path.cwd()/'docs/experiments/artifacts/2026-11-03-11'
user = Path.home()


def run(*command):
    with (artifact/'commands.jsonl').open('a') as log:
        log.write(json.dumps(list(map(str,command)))+'\n')
    return subprocess.check_output(list(map(str,command)),text=True,timeout=15).strip()


script = user/'.local/bin/register-3to2-mode'
assert script.read_bytes() == (artifact/'register-3to2-mode.py').read_bytes()
assert (user/'.config/display-modes/3to2.json').read_bytes() == (artifact/'mode.json').read_bytes()
ast.parse(script.read_text())
run('sh','-n',user/'.xprofile')
assert (user/'.xprofile').read_text().count('# BEGIN managed 3:2 display mode') == 1
assert 'kscreen' in run('qdbus6','org.kde.kded6','/kded','org.kde.kded6.loadedModules').splitlines()
unloaded = run('qdbus6','org.kde.kded6','/kded','org.kde.kded6.unloadModule','kscreen')
loaded = run('qdbus6','org.kde.kded6','/kded','org.kde.kded6.loadModule','kscreen')
assert loaded == 'true', loaded
time.sleep(1)
samples = []
for _ in range(3):
    data = json.loads(run('kscreen-doctor','--json'))
    sample = {}
    for output in data['outputs']:
        if not output['connected'] or not output['enabled']:
            continue
        mode = next(m for m in output['modes'] if m['id'] == output['currentModeId'])
        sample[output['name']] = {key:output[key] for key in ('pos','priority','rotation','scale')}
        sample[output['name']].update(size=mode['size'],refresh=mode['refreshRate'])
    assert sample['DP-1-1']['size'] == {'width':2544,'height':1696}, sample
    assert sample == json.loads((artifact/'applied.json').read_text())['outputs'], sample
    samples.append(sample)
    time.sleep(.5)
profile = user/'.local/share/kscreen/ed7b2f10d361607666c00173d2bb5cd8'
global_settings = user/'.local/share/kscreen/outputs/92acb37106a18c93620a455228b1f02d'
saved = json.loads(profile.read_text())
assert next(o for o in saved if o['metadata']['name']=='DP-1-1')['mode']['size']=={'width':2544,'height':1696}
assert json.loads(global_settings.read_text())['mode']['size']=={'width':2544,'height':1696}
report = {'kscreen_unloaded':unloaded,'kscreen_loaded':loaded,'samples':samples,
          'stored_layout_mode_verified':True,'stored_output_mode_verified':True,
          'login_action_sha256':hashlib.sha256(script.read_bytes()).hexdigest(),
          'xprofile_syntax':'passed','relogin_tested':False,'reboot_tested':False}
(artifact/'verified.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
