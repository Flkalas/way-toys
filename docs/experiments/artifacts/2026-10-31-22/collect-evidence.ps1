# Run from D:\Github\utils\way-toys after the final locked workspace checks.
$ErrorActionPreference = 'Stop'
$out = $PSScriptRoot
$paths = @('Cargo.toml', 'Cargo.lock') + @(rg --files crates | Where-Object { $_ -match '(\.rs$|Cargo\.toml$)' } | ForEach-Object { $_.Replace('\', '/') }) + @('scripts/zones-settings-vm.ps1', 'scripts/measure-zones-processes.py')
$pathsJson = $paths | ConvertTo-Json -Compress
$script = @'
import hashlib, json, os, pathlib, platform, subprocess
root = pathlib.Path('/home/debian/projects/way-toys')
paths = json.loads(r'''__PATHS__''')
def command(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
config = pathlib.Path('/home/debian/waytoys-test-results/settings-22/config')
result = {
    'utc': command('date', '-Is'),
    'os_release': pathlib.Path('/etc/os-release').read_text(),
    'kernel': platform.release(),
    'rustc': command('/home/debian/.cargo/bin/rustc', '--version'),
    'cargo': command('/home/debian/.cargo/bin/cargo', '--version'),
    'kwin': command('kwin_wayland', '--version'),
    'noto_package': command('dpkg-query', '-W', 'fonts-noto-cjk'),
    'noto_match': command('fc-match', 'Noto Sans CJK KR'),
    'cpu_count': os.cpu_count(),
    'memtotal': pathlib.Path('/proc/meminfo').read_text().splitlines()[0],
    'outputs': json.loads(command('kscreen-doctor', '-j')),
    'preferences': json.loads((config / 'preferences.json').read_text()),
    'layout_hashes': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((config / 'outputs').glob('*.json'))},
    'source_hashes': {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in paths},
    'units': {},
    'processes': []
}
for unit in ['waytoys-settings-21', 'waytoys-settings-21-reopen', 'waytoys-settings-22', 'waytoys-settings-22-invalid', 'waytoys-settings-22-final', 'waytoys-preview-22', 'waytoys-zones-settings-dev']:
    result['units'][unit] = command('systemctl', '--user', 'show', unit, '-p', 'MainPID', '-p', 'ActiveState')
for p in pathlib.Path('/proc').iterdir():
    if p.name.isdecimal():
        try:
            args = (p / 'cmdline').read_bytes().split(b'\0')
            if args and args[0].startswith(str(root / 'target/debug/waytoys-').encode()):
                result['processes'].append({'pid': int(p.name), 'args': [v.decode() for v in args if v]})
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
print(json.dumps(result, ensure_ascii=False, indent=2))
'@
$script = $script.Replace('__PATHS__', $pathsJson)
$encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($script))
$response = & ssh -o BatchMode=yes debian@192.168.50.125 "printf %s '$encoded' | base64 -d | XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-0 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus QT_QPA_PLATFORM=wayland python3"
if ($LASTEXITCODE -ne 0) { throw 'Guest evidence collection failed' }
$response | Set-Content -Encoding utf8 (Join-Path $out 'environment-final.json')
$guest = ($response -join "`n") | ConvertFrom-Json -AsHashtable
$comparison = foreach ($path in $paths) {
    $local = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    [pscustomobject]@{ path = $path; local_sha256 = $local; guest_sha256 = $guest.source_hashes[$path]; equal = $local -eq $guest.source_hashes[$path] }
}
$comparison | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $out 'source-hashes.json')
$mismatch = @($comparison | Where-Object { -not $_.equal })
if ($mismatch.Count) { $mismatch | Format-Table; throw 'Local and tested guest sources differ' }
Write-Output "Verified $($comparison.Count) source files; all local and guest SHA-256 hashes match."
Write-Output ($guest.processes | ConvertTo-Json -Depth 4)
