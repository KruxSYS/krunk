# Krunk 0.7.0 M7 (R12): "Send client hints". hints-serve.py on :8095 logs every request's
# Sec-CH-UA* headers and what the page, a cross-site iframe and its workers see as
# navigator.userAgentData. Runs: on (default), off (setting false), off in a private window,
# and switched off live on the settings page with the tab reloaded.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'hints'
$log = "$K\shots\hints-serve.log"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'hints-[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$K\hints-serve.py`"", '8095', "`"$log`""
Start-Sleep 2
function Run([string]$name, [string]$patch, [switch]$Private) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  $profile = "$K\p070h-$name"
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  if ($patch) {
    # A first start makes the profile; the patch goes in while Krunk is closed.
    Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
    Start-Sleep 7
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 3
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    # (Through a file: PowerShell mangles the quotes of an inline JSON argument.)
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
  Add-Content $log "=== $name ==="
  SafeGo 'http://127.0.0.1:8095/hints.html' 6000
  Shot "$P-$name"
  Copy-Item "$profile\Krunk\krunk.log" "$K\shots\$P-$name-krunk.log" -ErrorAction SilentlyContinue
}
Run 'on' ''
Run 'off' '{"headers": {"sendClientHints": false}}'
Run 'private-off' '{"headers": {"sendClientHints": false}}' -Private
# Live: on at start, switched off in Settings (search "client hints"), then the page reloaded.
Run 'live' ''
SafeGo 'krunk://settings' 2500
$c = ClientRect (MainWindow)
Click ([int]($c.Left + 968 * $c.Scale)) ([int]($c.Top + 146 * $c.Scale)) 300; Keys 'send client hints' 1500
Shot "$P-live-setting"
Log "[$P-live] (click the switch on the screenshot's row next)"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] server log:"
Get-Content $log | ForEach-Object { Log "  $_" }
Log "[$P] done"
