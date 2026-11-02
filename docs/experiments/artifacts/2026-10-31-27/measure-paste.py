#!/usr/bin/env python3
"""Read-only measurements of the three explicitly named Waytoys D-Bus owners."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

def sample(pid):
    root=Path('/proc')/str(pid)
    status=dict(line.split(':',1) for line in (root/'status').read_text().splitlines())
    memory=dict(line.split(':',1) for line in (root/'smaps_rollup').read_text().splitlines()[1:])
    stat=(root/'stat').read_text().rsplit(') ',1)[1].split()
    return dict(rss_kib=int(status['VmRSS'].split()[0]),peak_rss_kib=int(status['VmHWM'].split()[0]),pss_kib=int(memory['Pss'].split()[0]),threads=int(status['Threads']),cpu_ticks=int(stat[11])+int(stat[12]))

rows=[]
for name in ['org.waytoys.Runtime1','org.waytoys.Paste1','org.waytoys.PastePicker1']:
    pid=int(subprocess.check_output(['qdbus6','org.freedesktop.DBus','/','org.freedesktop.DBus.GetConnectionUnixProcessID',name]))
    rows.append(dict(name=name,pid=pid,before=sample(pid)))
start=time.monotonic()
time.sleep(3)
elapsed=time.monotonic()-start
for row in rows:
    row['after']=sample(row['pid'])
    row['cpu_seconds']=(row['after']['cpu_ticks']-row['before']['cpu_ticks'])/os.sysconf('SC_CLK_TCK')
print(json.dumps(dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),elapsed_seconds=elapsed,processes=rows),indent=2))
