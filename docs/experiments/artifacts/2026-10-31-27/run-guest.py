#!/usr/bin/env python3
"""Run a preserved UTF-8 shell script through SSH without PowerShell CRLF pipes."""
from pathlib import Path
import subprocess
import sys

script,log=map(Path,sys.argv[1:3])
result=subprocess.run(['ssh','debian@192.168.50.125','bash -s'],input=script.read_bytes().replace(b'\r\n',b'\n'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
log.write_bytes(result.stdout)
print(result.stdout.decode(errors='replace')[-5000:])
sys.exit(result.returncode)
