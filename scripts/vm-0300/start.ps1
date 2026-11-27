# Krunk 0.30.0 cast smoke (VM, run through the interactive task): a fresh profile for build -App
# (or -KeepProfile), optional settings (-Set "casting=false"), then -Url. The probe page is served
# from C:\pipeline\krunk\v0300 on port 18112. -Dev: KRUNK_DEVTOOLS_PORT=9222 (fast builds).
param([string]$App = 'app300', [string]$Prof = 'c1', [string]$Url = 'about:blank', [switch]$Dev, [switch]$KeepProfile, [string]$Set = '')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'; $V = "$K\v0300"; $Py = 'C:\Program Files\Python312\python.exe'
$Shots = "$V\shots"; New-Item -ItemType Directory -Force $Shots | Out-Null
if (-not (Get-NetTCPConnection -LocalPort 18112 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process -FilePath $Py -ArgumentList '-m', 'http.server', '18112', '--bind', '127.0.0.1', '--directory', $V -WindowStyle Hidden
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$P = "$K\p0300-$Prof"
function StartKrunk {
  $env:LOCALAPPDATA = $P
  if ($Dev) { $env:KRUNK_DEVTOOLS_PORT = '9222' } else { Remove-Item Env:KRUNK_DEVTOOLS_PORT -ErrorAction SilentlyContinue }
  Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 5
  [U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
  [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 800
}
if (-not $KeepProfile) {
  Remove-Item -Recurse -Force $P -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $P | Out-Null
  StartKrunk
  Start-Sleep 6
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
}
if ($Set) {
  $out = & $Py "$V\setprof.py" "$P\Krunk\browser.db" @($Set.Split(' ')) 2>&1
  Log "setprof $Set -> $out"
}
StartKrunk
Click 640 30 300
# A fast build shows the Repair popup: Later.
Start-Sleep 14; Click 1127 175 800; Click 640 30 300
if ($Url -ne 'about:blank') { Go $Url 2500 }
Log "krunk $App $Prof $Url set=[$Set]"
