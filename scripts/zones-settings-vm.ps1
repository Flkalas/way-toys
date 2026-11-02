[CmdletBinding()]
param(
    [ValidateSet('Check','Start','Input','Clipboard','Capture','Status','Stop')][string]$Action,
    [string]$Guest = 'debian@192.168.50.125',
    [string]$PveHost = 'INOPS-PVE',
    [int]$VmId = 700,
    [string]$GuestRepo = '/home/debian/projects/way-toys',
    [string]$ConfigDir = '/home/debian/waytoys-test-results/settings-21/config',
    [string]$Unit = 'waytoys-settings-21',
    [string]$Events,
    [string]$ClipboardFile,
    [int]$DesktopWidth = 1962,
    [int]$DesktopHeight = 627,
    [string]$OutputDirectory = 'docs\experiments\artifacts\2026-10-31-21'
)
$ErrorActionPreference='Stop'
if ($Unit -notmatch '^waytoys-[a-zA-Z0-9-]+$') { throw 'Invalid test unit name' }
function Encode([string]$Value) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value)) }
function Remote([string]$Script) {
    $encoded=Encode $Script
    & ssh -o BatchMode=yes $Guest "printf %s '$encoded' | base64 -d | bash"
    if ($LASTEXITCODE -ne 0) { throw "Guest command failed: $LASTEXITCODE" }
}
$prefix=@'
set -eu
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
repo=$(printf %s '__REPO__' | base64 -d)
config=$(printf %s '__CONFIG__' | base64 -d)
cd "$repo"
'@
$prefix=$prefix.Replace('__REPO__',(Encode $GuestRepo)).Replace('__CONFIG__',(Encode $ConfigDir))+"`n"
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
switch ($Action) {
    Check {
        Remote ($prefix+@'
. "$HOME/.cargo/env"
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo tree -p waytoys-zones-settings -e features
target/debug/waytoys-zones-editor --list-outputs --json
'@) | Tee-Object -FilePath (Join-Path $OutputDirectory 'validation.log')
    }
    Start {
        Remote ($prefix+"sudo -n loginctl unlock-session 1`nsystemd-run --user --unit='$Unit' --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=WINIT_UNIX_BACKEND=wayland --setenv=ICED_BACKEND=tiny-skia `"`$repo/target/debug/waytoys-zones-settings`" --config-dir `"`$config`"")
    }
    Input {
        if (-not $Events) { throw '-Events JSON path is required' }
        $json=Get-Content -LiteralPath $Events -Raw
        $null=$json|ConvertFrom-Json
        & scp (Join-Path $PSScriptRoot 'vm-input.py') "${PveHost}:/tmp/waytoys-settings-vm-input.py"
        if ($LASTEXITCODE -ne 0) { throw 'Input helper upload failed' }
        $encoded=Encode $json
        & ssh -o BatchMode=yes $PveHost "printf %s '$encoded' | base64 -d | python3 /tmp/waytoys-settings-vm-input.py --vm-id $VmId --width $DesktopWidth --height $DesktopHeight"
        if ($LASTEXITCODE -ne 0) { throw 'VM input failed' }
    }
    Clipboard {
        if (-not $ClipboardFile) { throw '-ClipboardFile UTF-8 path is required' }
        $encoded=Encode (Get-Content -LiteralPath $ClipboardFile -Raw)
        # wl-copy forks; close inherited SSH descriptors in its owner process.
        Remote ($prefix+"printf %s '$encoded' | base64 -d | wl-copy >/dev/null 2>&1")
    }
    Capture {
        & (Join-Path $PSScriptRoot 'capture-vm.ps1') -Guest $Guest -PveHost $PveHost -VmId $VmId -GuestRepo $GuestRepo -OutputDirectory $OutputDirectory -NoOpen
    }
    Status {
        Remote ($prefix+@'
date -Is
kscreen-doctor -j
ps -eo pid,ppid,comm,rss,etime,args | grep '[t]arget/debug/waytoys-zones-'
if [ -f "$config/preferences.json" ]; then cat "$config/preferences.json"; fi
if [ -d "$config/outputs" ]; then find "$config/outputs" -type f -exec sha256sum {} +; fi
'@)
    }
    Stop { Remote ($prefix+"systemctl --user stop '$Unit'`njournalctl --user -u '$Unit' --no-pager") }
}
