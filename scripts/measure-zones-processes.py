#!/usr/bin/env python3
"""Read-only /proc sample of the explicitly named development user units."""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("units", nargs="+")
parser.add_argument("--seconds", type=float, default=10)
args = parser.parse_args()
if not 0 < args.seconds <= 60 or any(not (unit.startswith("waytoys-") or unit == "waytoys.service") for unit in args.units):
    parser.error("use explicit waytoys units and an interval of 0–60 seconds")

def sample(pid):
    root = Path("/proc") / str(pid)
    status = dict(line.split(":", 1) for line in (root / "status").read_text().splitlines())
    stat = (root / "stat").read_text().rsplit(") ", 1)[1].split()
    rollup = (root / "smaps_rollup").read_text().splitlines()[1:]
    memory = dict(line.split(":", 1) for line in rollup)
    return {"rss_kib": int(status["VmRSS"].split()[0]),
            "peak_rss_kib": int(status["VmHWM"].split()[0]),
            "pss_kib": int(memory["Pss"].split()[0]),
            "threads": int(status["Threads"]),
            "cpu_ticks": int(stat[11]) + int(stat[12])}

rows = []
for unit in args.units:
    pid = int(subprocess.check_output(["systemctl", "--user", "show", unit, "-p", "MainPID", "--value"], text=True))
    rows.append({"unit": unit, "pid": pid, "before": sample(pid) if pid else None})
start = time.monotonic()
time.sleep(args.seconds)
elapsed = time.monotonic() - start
for row in rows:
    if row["pid"]:
        row["after"] = sample(row["pid"])
        row["cpu_seconds"] = (row["after"]["cpu_ticks"] - row["before"]["cpu_ticks"]) / os.sysconf("SC_CLK_TCK")
print(json.dumps({"utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  "elapsed_seconds": elapsed, "processes": rows}, indent=2))
