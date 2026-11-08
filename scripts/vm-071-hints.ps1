# Krunk 0.7.1 (R12): "Send client hints" off reaches every renderer. hints-serve.py on :8095 logs
# every request's Sec-CH-UA* headers and what the page, a cross-site iframe and the workers see as
# navigator.userAgentData. Rows:
#   on, off, private-off   fresh profiles, as in 0.7.0
#   live                   on at start; a tab with the test page (polling) stays open; the
#                          setting is switched off in a second tab; the open tab is looked at
#                          without a reload ("page later", "iframe later"), then reloaded 5 times
#   live-on                switched on again; the tab reloaded
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'hints071'
$log = "$K\shots\hints-serve.log"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'hints-[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$K\hints-serve.py`"", '8095', "`"$log`""
Start-Sleep 2
function Mark([string]$what) { Add-Content $log "=== $what ===" }
function Run([string]$name, [string]$patch, [switch]$Private) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  $profile = "$K\p071h-$name"
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  if ($patch) {
    Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
    Start-Sleep 7
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 3
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Set-Content "$K\hints-patch.json" $patch
    Log "[$P-$name] settings: $(python "$K\setkv.py" $profile "@$K\hints-patch.json" 2>&1)"
  }
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
  if ($Private) {
    SafeKeys '^+n' 3000
    $B = (Frames | Where-Object { $_ -ne $A } | Select-Object -First 1)
    if ($B) { [U]::MoveWindow($B, 0, 0, 1280, 760, $true) | Out-Null; Focus $B | Out-Null }
  }
  $script:profile = $profile
}
function Toggle([string]$what) {
  # Settings in its own tab: search "client hints", click the switch.
  SafeKeys '^t' 1500
  SafeGo 'krunk://settings' 2500
  $c = ClientRect (MainWindow)
  Click ([int]($c.Left + 968 * $c.Scale)) ([int]($c.Top + 146 * $c.Scale)) 300; Keys 'send client hints' 1500
  Click 1106 349 1200
  Shot "$P-$what-setting"
  SafeKeys '^w' 1500
}

foreach ($row in @(@('on', ''), @('off', '{"headers": {"sendClientHints": false}}'))) {
  Run $row[0] $row[1]
  Mark $row[0]
  SafeGo 'http://127.0.0.1:8095/hints.html' 7000
  Shot "$P-$($row[0])"
  Copy-Item "$profile\Krunk\krunk.log" "$K\shots\$P-$($row[0])-krunk.log" -ErrorAction SilentlyContinue
}
Run 'private-off' '{"headers": {"sendClientHints": false}}' -Private
Mark 'private-off'
SafeGo 'http://127.0.0.1:8095/hints.html' 7000
Shot "$P-private-off"

# Live: the test page stays open in tab 1 while the setting changes in tab 2.
Run 'live' ''
Mark 'live: on, tab open'
SafeGo 'http://127.0.0.1:8095/hints.html?poll' 7000
Toggle 'off'
Mark 'live: switched off, the open tab without a reload (next 8 s)'
Start-Sleep 8
Shot "$P-live-open-tab"
for ($n = 1; $n -le 5; $n++) {
  Mark "live: off, reload $n"
  SafeKeys '{F5}' 6000
}
Shot "$P-live-reloaded"
Toggle 'on'
Mark 'live-on: switched on again, reload'
SafeKeys '{F5}' 6000
Shot "$P-live-on"
Copy-Item "$profile\Krunk\krunk.log" "$K\shots\$P-live-krunk.log" -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] server log:"
Get-Content $log | Where-Object { $_ -match '===|REPORT' } | ForEach-Object { Log "  $_" }
Log "[$P] done"
