#!/usr/bin/env python3
"""Check syntax, record source hashes, compare exact local/guest source bytes."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess

root=Path(__file__).resolve().parents[4]
artifacts=Path(__file__).parent
paths=[root/n for n in ['Cargo.toml','Cargo.lock','README.md','rust-toolchain.toml','docs/paste.ko.md','docs/capture.ko.md','docs/zones-runtime.ko.md','docs/zones-settings.ko.md','docs/experiments/2026-10-31-27.md']]
paths+=list((root/'crates').rglob('*.rs'))+list((root/'crates').rglob('Cargo.toml'))
paths+=list((root/'scripts').glob('*.py'))+list((root/'scripts').glob('*.cpp'))+list((root/'scripts').glob('*.sh'))
for path in (root/'scripts').glob('*.py'):ast.parse(path.read_text(encoding='utf-8-sig'),filename=str(path))
for path in artifacts.glob('*.py'):ast.parse(path.read_text(encoding='utf-8-sig'),filename=str(path))
subprocess.run(['git','diff','--check'],cwd=root,check=True)
manifest={p.relative_to(root).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(paths))}
(artifacts/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
script='''import hashlib,json
from pathlib import Path
root=Path('/home/debian/projects/way-toys')
manifest=json.loads(MANIFEST)
different=[name for name,digest in manifest.items() if hashlib.sha256((root/name).read_bytes()).hexdigest()!=digest]
assert not different,different
print(f'Exact local/guest SHA-256 match: {len(manifest)} source files')
'''.replace('MANIFEST',repr(json.dumps(manifest)))
result=subprocess.run(['ssh','debian@192.168.50.125','python3 -'],input=script.encode(),stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=30)
assert result.returncode==0,result.stdout.decode(errors='replace')
syntax=subprocess.run(['ssh','debian@192.168.50.125','sh -n /home/debian/projects/way-toys/scripts/install-paste.sh'],capture_output=True,timeout=30)
assert syntax.returncode==0,syntax.stderr.decode(errors='replace')
log='Python AST: OK\ninstall-paste.sh syntax: OK\ngit diff --check: OK\n'+result.stdout.decode()
(artifacts/'static-check.log').write_text(log)
print(log)
