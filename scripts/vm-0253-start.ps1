# Krunk 0.25.3 VM pass: starts the fast build (C:\pipeline\krunk\app253) with the DevTools port
# 9222 and leaves it running; scripts/perf/d0253.mjs drives it over CDP (ssh -L 19870:...:9222).
#   -Fresh: a new profile.
param([switch]$Fresh)
. C:\pipeline\krunk\lib.ps1
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0253"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 2
if ($Fresh) { Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $prof | Out-Null }
if (-not (Get-NetTCPConnection -State Listen -LocalPort 8094 -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$Root\serve.ps1", '-PrefixList', 'http://*:8094/'
}
$env:LOCALAPPDATA = $prof; $env:KRUNK_DEVTOOLS_PORT = '9222'
Start-Process -FilePath "$Root\app253\Krunk.exe" -WorkingDirectory "$Root\app253"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
Start-Sleep 22
Click 1127 175 500   # a fast build's "repair" prompt: Later
Log "started (fresh=$Fresh)"
