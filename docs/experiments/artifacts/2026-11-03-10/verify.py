"""Run from the repository root; verify the final X11 evidence without GUI input."""
import ast
import hashlib
import json
from pathlib import Path
import re
import subprocess

root = Path.cwd()
artifact = root / 'docs/experiments/artifacts/2026-11-03-10'
results = {}
for name in ('scripts/test-x11.py', 'scripts/x11-session-fixture.py'):
    ast.parse((root / name).read_text(), filename=name)
results['python_syntax'] = 'passed'
changed = subprocess.check_output(['git', 'diff', '--name-only'], text=True).splitlines()
changed += subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], text=True).splitlines()
changed += subprocess.check_output(['git', 'diff', '--cached', '--name-only'], text=True).splitlines()
missing = []
for name in changed:
    file = root / name
    if file.suffix != '.md':
        continue
    for link in re.findall(r'\]\(([^)]+)\)', file.read_text()):
        target = link.split('#', 1)[0]
        if target and not re.match(r'\w+://', target) and not (file.parent / target).exists():
            missing.append([name, target])
assert not missing, missing
results['markdown_links'] = 'passed'
manifest = json.loads((artifact / 'integration/source-manifest.json').read_text())
normalized = []
for name, digest in manifest.items():
    data = (root / name).read_bytes()
    if name == 'crates/waytoys-desktop/Cargo.toml' and hashlib.sha256(data).hexdigest() != digest:
        before = (artifact / 'desktop-Cargo.before.toml').read_bytes()
        assert hashlib.sha256(before).hexdigest() == digest
        assert data == before.rstrip() + b'\n'
        normalized.append(name)
    else:
        assert hashlib.sha256(data).hexdigest() == digest, name
results['integration_source_manifest_files'] = len(manifest)
results['post_test_whitespace_only'] = normalized
old = subprocess.check_output(['git', 'show', '47bd992:crates/waytoys-runtime/src/overlay.rs'])
assert old == (root / 'crates/waytoys-runtime/src/wayland_overlay.rs').read_bytes()
results['wayland_overlay_unchanged'] = True
results['tests_passed'] = sum(map(int, re.findall(r'test result: ok\. (\d+) passed', (artifact / 'test.log').read_text())))
assert json.loads((artifact / 'integration/results.json').read_text())['passed']
(artifact / 'static-verification.json').write_text(json.dumps(results, indent=2) + '\n')
print(json.dumps(results))
