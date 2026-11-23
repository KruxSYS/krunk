# Krunk 0.28.0 harness (VM, run through the interactive task): a fresh profile for build -App at
# protection level -Level, then -Url. -Chrome starts the VM's real Chrome (fresh profile)
# instead. The probe server (probe-serve.py, port 18111) is started when it isn't running.
#   -Dev: KRUNK_DEVTOOLS_PORT=9222 (fast builds only; diagnostic runs, never the clean ones).
param([string]$App = 'app271r', [string]$Prof = 'x', [string]$Level = 'standard', [string]$Url = 'about:blank',
  [switch]$Chrome, [switch]$Dev, [switch]$KeepProfile, [string]$Set = '', [string]$PerfOff = '', [string]$ChromeArgs = '')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'; $V = "$K\v0280"; $Py = 'C:\Program Files\Python312\python.exe'
if (-not (Get-NetTCPConnection -LocalPort 18111 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process -FilePath $Py -ArgumentList "$V\probe-serve.py", '18111' -WorkingDirectory $V -WindowStyle Hidden
  Start-Sleep 2
}
# A leftover Windows Firewall prompt takes the keystrokes: Esc it.
for ($i = 0; $i -lt 3; $i++) {
  $fw = [U]::FindWindow([NullString]::Value, 'Windows Security')
  if ($fw -eq [IntPtr]::Zero) { break }
  [void][U]::SetForegroundWindow($fw); Start-Sleep -Milliseconds 500; Keys '{ESC}' 1000
}
Get-Process Krunk, chrome -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$P = "$K\p0280-$Prof"
if ($Chrome) {
  if (-not $KeepProfile) { Remove-Item -Recurse -Force $P -ErrorAction SilentlyContinue }
  Start-Process -FilePath 'C:\Program Files\Google\Chrome\Application\chrome.exe' -ArgumentList "--user-data-dir=$P", '--no-first-run', '--no-default-browser-check', '--window-position=0,0', '--window-size=1280,760', $(if ($Dev) { '--remote-debugging-port=9222' } else { '--no-first-run' }), $(if ($ChromeArgs) { $ChromeArgs } else { '--no-first-run' }), $Url
  Start-Sleep 8
  $h = MainWindow 'Chrome_WidgetWin_1'
  if ($h -ne [IntPtr]::Zero) { [U]::MoveWindow($h, 0, 0, 1280, 760, $true) | Out-Null }
  Log "chrome $Prof $Url"
  return
}
function StartKrunk {
  $env:LOCALAPPDATA = $P
  # perf-probe builds: page scripts to leave out (KRUNK_PERF_OFF, e.g. pointer,notify).
  if ($PerfOff) { $env:KRUNK_PERF_OFF = $PerfOff } else { Remove-Item Env:KRUNK_PERF_OFF -ErrorAction SilentlyContinue }
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
  $args2 = @("protection.level=`"$Level`"", "blockThirdPartyCookies=$(if ($Level -eq 'strict') { 'true' } else { 'false' })")
  if ($Set) { $args2 += $Set.Split(' ') }
  $out = & $Py "$V\setprof.py" "$P\Krunk\browser.db" @args2 2>&1
  $chk = & $Py "$V\setprof.py" "$P\Krunk\browser.db" show protection
  Log "setprof: $out / settings now: $chk"
}
StartKrunk
Click 640 30 300
# A fast build carrying a published version shows the Repair popup: Later.
if ($App -notlike '*r') { Start-Sleep 14; Click 1127 175 800; Click 640 30 300 }
if ($Url -ne 'about:blank') { Go $Url 1500 }
Log "krunk $App $Prof $Level $Url"
