#!/usr/bin/python3
"""Install corrected Debian login registration and the X11 session recovery service."""
import ast
import datetime
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile

ART=Path.cwd()/'docs/experiments/artifacts/2026-11-04-01'
USER=Path.home()


def run(*args):
    with (ART/'commands.jsonl').open('a') as log:
        log.write(json.dumps(list(map(str,args)))+'\n')
    return subprocess.check_output(list(map(str,args)),text=True,stderr=subprocess.STDOUT,timeout=40)


def write(path,data,mode):
    path.parent.mkdir(parents=True,exist_ok=True)
    fd,temporary=tempfile.mkstemp(dir=path.parent,prefix='.display-mode-')
    try:
        with os.fdopen(fd,'wb') as f:
            f.write(data);f.flush();os.fsync(f.fileno())
        os.chmod(temporary,mode);os.replace(temporary,path)
    finally:
        if Path(temporary).exists():Path(temporary).unlink()


assert os.environ.get('XDG_SESSION_TYPE')=='x11'
backup=USER/'.local/state/display-modes/backups'/datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
backup.mkdir(parents=True,mode=0o700)
targets=[USER/'.xprofile',USER/'.xsessionrc',USER/'.local/bin/register-3to2-mode',USER/'.config/systemd/user/display-3to2.service']
for path in targets:
    if path.is_symlink():raise RuntimeError(f'Preserved symlink {path}')
    if path.exists():shutil.copy2(path,backup/path.name)
shutil.copytree(USER/'.local/share/kscreen',backup/'kscreen',symlinks=True)
script=ART/'register-3to2-mode.py'
ast.parse(script.read_text())
write(USER/'.local/bin/register-3to2-mode',script.read_bytes(),0o755)
profile=USER/'.xprofile'
text=profile.read_text()
text=re.sub(r'\n?# BEGIN managed 3:2 display mode\n.*?# END managed 3:2 display mode\n?', '\n',text,flags=re.S)
write(profile,text.encode(),stat.S_IMODE(profile.stat().st_mode))
sessionrc=USER/'.xsessionrc'
original=sessionrc.read_text() if sessionrc.exists() else ''
original=re.sub(r'\n?# BEGIN managed 3:2 display mode\n.*?# END managed 3:2 display mode\n?', '\n',original,flags=re.S)
hook='''
# BEGIN managed 3:2 display mode
# Debian /etc/sddm/Xsession -> /etc/X11/Xsession.d/40x11-common_xsessionrc.
if [ -x "$HOME/.local/bin/register-3to2-mode" ]; then
    mkdir -p "$HOME/.local/state/display-modes"
    "$HOME/.local/bin/register-3to2-mode" >> "$HOME/.local/state/display-modes/3to2-login.log" 2>&1 || :
fi
# END managed 3:2 display mode
'''
write(sessionrc,(original.rstrip()+'\n'+hook).encode(),0o644)
for path in (profile,sessionrc):run('sh','-n',path)
service=USER/'.config/systemd/user/display-3to2.service'
write(service,(ART/'display-3to2.service').read_bytes(),0o644)
run('systemd-analyze','--user','verify',service)
run('systemctl','--user','daemon-reload')
run('systemctl','--user','enable','--now','display-3to2.service')
# Exercise the same sourcing step that the active Debian Xsession uses.
run('sh','-c','USERXSESSIONRC="$HOME/.xsessionrc"; . /etc/X11/Xsession.d/40x11-common_xsessionrc')
report={'time':datetime.datetime.now().astimezone().isoformat(),'backup':str(backup),
        'service_enabled':run('systemctl','--user','is-enabled','display-3to2.service').strip(),
        'service_active':run('systemctl','--user','is-active','display-3to2.service').strip(),
        'xsessionrc_path':str(sessionrc),'script_path':str(USER/'.local/bin/register-3to2-mode')}
(ART/'installed.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
