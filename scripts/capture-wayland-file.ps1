[CmdletBinding()]
param(
    [string]$Guest='debian@192.168.50.125',
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference='Stop'
$destination=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $destination | Out-Null
if (Test-Path (Join-Path $destination 'screen.png')) { throw 'Choose a new output directory' }
$remote='/tmp/waytoys-direct-'+[guid]::NewGuid().ToString('N')
$script=@'
set -eu
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export WAYLAND_DISPLAY=$(systemctl --user show-environment | sed -n 's/^WAYLAND_DISPLAY=//p')
export QT_QPA_PLATFORM=wayland
mkdir -p __OUT__
date -Is > __OUT__/time.log
kscreen-doctor -j > __OUT__/outputs.json
timeout 10s spectacle --background --fullscreen --nonotify --output __OUT__/screen.png > __OUT__/capture.log 2>&1
test -s __OUT__/screen.png
sha256sum __OUT__/screen.png > __OUT__/sha256.txt
'@
$encoded=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($script.Replace('__OUT__',$remote).Replace("`r`n","`n")))
& ssh -o BatchMode=yes $Guest "printf %s '$encoded' | base64 -d | bash"
if ($LASTEXITCODE -ne 0) { throw 'Native Wayland capture failed' }
& scp "${Guest}:$remote/*" $destination
if ($LASTEXITCODE -ne 0) { throw 'Capture download failed' }
$file=Join-Path $destination 'screen.png'
$hash=(Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant()
if ((Get-Content (Join-Path $destination 'sha256.txt')) -notlike "$hash *") { throw 'Capture hash mismatch' }
[pscustomobject]@{method='spectacle-native-background-file';path=$file;sha256=$hash}|ConvertTo-Json|Set-Content (Join-Path $destination 'manifest.json')
Write-Output $file
