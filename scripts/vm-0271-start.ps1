# Krunk 0.27.1 VM pass: starts the fast build (C:\pipeline\krunk\app271) with the DevTools port
# 9222 and leaves it running; scripts/perf/d0271.mjs drives it over CDP (ssh -L 19870:...:9222).
#   -Fresh: a new profile.
param([switch]$Fresh)
. C:\pipeline\krunk\lib.ps1
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0271"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 2
if ($Fresh) { Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $prof | Out-Null }
$env:LOCALAPPDATA = $prof; $env:KRUNK_DEVTOOLS_PORT = '9222'
Start-Process -FilePath "$Root\app271\Krunk.exe" -WorkingDirectory "$Root\app271"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
Start-Sleep 22
Click 1127 175 500   # a fast build's "repair" prompt: Later
Log "started (fresh=$Fresh)"
