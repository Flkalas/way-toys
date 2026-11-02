[CmdletBinding()]
param(
    [string]$Guest = 'debian@192.168.50.125',
    [string]$PveHost = 'INOPS-PVE',
    [ValidateRange(100, 999999999)][int]$VmId = 700,
    [string]$GuestRepo = '/home/debian/projects/way-toys',
    [string]$OutputDirectory,
    [switch]$Overlay,
    [switch]$Build,
    [switch]$ClipboardOnly,
    [switch]$NoOpen
)

$ErrorActionPreference = 'Stop'
if ($ClipboardOnly -and ($Overlay -or $Build)) { throw '-ClipboardOnly cannot be combined with -Overlay or -Build.' }
foreach ($command in 'ssh', 'scp') { Get-Command $command -ErrorAction Stop | Out-Null }
$repoRoot = Split-Path $PSScriptRoot -Parent
$runId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 6)
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repoRoot ".artifacts\captures\$runId" }
$outputPath = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
if (Test-Path (Join-Path $outputPath 'clipboard.png')) { throw 'Output directory already contains clipboard.png; choose a new directory.' }
$remoteDir = "/tmp/waytoys-capture-$runId"
$sshOptions = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8')
function Encode-Text([string]$Value) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value)) }
$context = @'
set -eu
repo=$(printf %s '__REPO__' | base64 -d)
out=__OUT__
overlay_unit=waytoys-capture-overlay-__ID__
editor_unit=waytoys-capture-editor-__ID__
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export WAYLAND_DISPLAY=$(systemctl --user show-environment | sed -n 's/^WAYLAND_DISPLAY=//p')
export QT_QPA_PLATFORM=wayland
[ -n "$WAYLAND_DISPLAY" ] && [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || { echo 'No active Wayland session' >&2; exit 1; }
mkdir -p "$out"
'@
$context = $context.Replace('__REPO__', (Encode-Text $GuestRepo)).Replace('__OUT__', $remoteDir).Replace('__ID__', $runId)
function Invoke-Guest([string]$Script, [switch]$AllowFailure) {
    $encoded = Encode-Text ($context + "`n" + $Script)
    $result = & ssh @sshOptions $Guest "echo $encoded | base64 -d | bash" 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not $AllowFailure) { throw "Guest command failed ($code): $($result -join [Environment]::NewLine)" }
    [pscustomobject]@{ ExitCode = $code; Text = ($result -join [Environment]::NewLine) }
}
$prepared = $false
try {
    if (-not $ClipboardOnly) {
        $prepared = $true
        $prepare = @'
for cmd in spectacle wl-paste wl-copy kscreen-doctor python3; do command -v "$cmd" >/dev/null; done
if pgrep -x spectacle >/dev/null; then echo 'Close the existing Spectacle window before capturing, or use -ClipboardOnly.' >&2; exit 1; fi
session=$(loginctl show-user "$(id -u)" -p Display --value)
if [ -n "$session" ] && [ "$(loginctl show-session "$session" -p LockedHint --value)" = yes ]; then sudo -n loginctl unlock-session "$session"; fi
date -Is > "$out/time.log"
kscreen-doctor -j > "$out/outputs-before.json"
git -C "$repo" rev-parse HEAD > "$out/commit.log"
git -C "$repo" status --short > "$out/source-status.log"
if [ __BUILD__ = 1 ]; then . "$HOME/.cargo/env"; cd "$repo"; cargo build --workspace --locked > "$out/build.log" 2>&1; fi
if [ __OVERLAY__ = 1 ]; then
    systemd-run --user --unit="$overlay_unit" --collect "$repo/target/debug/waytoys-overlay" --display
    sleep 1
    systemctl --user is-active --quiet "$overlay_unit"
fi
wl-copy --clear
systemd-run --user --unit="$editor_unit" --collect /usr/bin/spectacle --fullscreen --gui
'@
        $prepare = $prepare.Replace('__BUILD__', [int]$Build.IsPresent).Replace('__OVERLAY__', [int]$Overlay.IsPresent)
        $null = Invoke-Guest $prepare
        Start-Sleep -Seconds 2
    }
    $export = @'
timeout 3s wl-paste --list-types > "$out/clipboard-types.log" 2>/dev/null || exit 3
grep -Fx 'image/png' "$out/clipboard-types.log" >/dev/null || exit 3
timeout 5s wl-paste --type image/png > "$out/clipboard.png"
python3 - "$out" <<'PY'
import hashlib, json, struct, sys
from pathlib import Path
p = Path(sys.argv[1])
data = (p / 'clipboard.png').read_bytes()
if len(data) < 33 or data[:8] != b'\x89PNG\r\n\x1a\n':
    raise SystemExit('Clipboard image is not a PNG')
width, height = struct.unpack('>II', data[16:24])
manifest = dict(method='guest-wayland-clipboard', width=width, height=height,
                bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
(p / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
PY
kscreen-doctor -j > "$out/outputs-after.json"
'@
    $captured = $false
    $attempts = if ($ClipboardOnly) { 1 } else { 4 }
    for ($attempt = 0; $attempt -lt $attempts; $attempt++) {
        if (-not $ClipboardOnly) {
            $keyResult = & ssh @sshOptions $PveHost "printf 'sendkey ctrl-c\nquit\n' | qm monitor $VmId" 2>&1
            if ($LASTEXITCODE -ne 0) { throw "Failed to send Copy to VM $VmId`: $($keyResult -join ' ')" }
            Start-Sleep -Milliseconds 600
        }
        $exportResult = Invoke-Guest $export -AllowFailure
        if ($exportResult.ExitCode -eq 0) { $captured = $true; break }
        if ($exportResult.ExitCode -ne 3) { throw $exportResult.Text }
        Start-Sleep -Milliseconds 500
    }
    if (-not $captured) { throw 'No PNG reached the guest clipboard. Do not use the VM keyboard during capture; check Spectacle focus.' }
    & scp @sshOptions "${Guest}:$remoteDir/*" $outputPath
    if ($LASTEXITCODE -ne 0) { throw 'Failed to download capture artifacts.' }
    $manifest = Get-Content (Join-Path $outputPath 'manifest.json') -Raw | ConvertFrom-Json
    $imagePath = Join-Path $outputPath 'clipboard.png'
    $hash = (Get-FileHash $imagePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $manifest.sha256) { throw 'Downloaded PNG hash does not match the guest clipboard image.' }
    $pageId = $null
    if (-not $NoOpen) {
        $orcaCommand = if ($env:ORCA_CLI_COMMAND) { $env:ORCA_CLI_COMMAND } elseif ($env:ORCA_DEV_REPO_ROOT) { 'orca-dev' } else { 'orca' }
        Get-Command $orcaCommand -ErrorAction Stop | Out-Null
        $url = ([uri]$imagePath).AbsoluteUri
        $tab = (& $orcaCommand tab create --url $url --json | ConvertFrom-Json)
        if ($LASTEXITCODE -ne 0 -or -not $tab.ok) { throw "Capture saved at $imagePath, but opening the Orca tab failed." }
        $pageId = $tab.result.browserPageId
    }
    [pscustomobject]@{ ImagePath = $imagePath; Width = $manifest.width; Height = $manifest.height; Sha256 = $hash; BrowserPageId = $pageId }
} finally {
    if ($prepared) {
        $cleanup = @'
journalctl --user -u "$overlay_unit" --no-pager > "$out/overlay.log" 2>&1 || true
journalctl --user -u "$editor_unit" --no-pager > "$out/spectacle.log" 2>&1 || true
systemctl --user stop "$editor_unit" "$overlay_unit" 2>/dev/null || true
'@
        $cleanupResult = Invoke-Guest $cleanup -AllowFailure
        if ($cleanupResult.ExitCode -ne 0) { Write-Warning "Capture cleanup failed; guest artifacts: $remoteDir" }
        & scp @sshOptions "${Guest}:$remoteDir/*.log" $outputPath 2>$null
    }
}
