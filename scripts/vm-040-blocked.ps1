# Krunk 0.4.0 milestone 7: what the ad blocker blocked. vm-www/8092/ads.html loads scripts and
# pixels from ad/tracker hosts; the shield panel lists them, "Show all" opens krunk://adblock
# with the filter and list of each. Started with --log-blocked: the log's lines are copied for
# the comparison. Fresh profile. Screenshots "blk-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'blk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p040k'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -ArgumentList '--log-blocked' -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
SafeGo 'http://localhost:8092/ads.html' 4000
Shot "$P-1-page"
# The shield (toolbar, left of the menu button).
Click ([int]($c.Right - 56 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 1500
Shot "$P-2-shield"
# Tab to "Show all" (after "Allow ads on this site" and "Cookies and site data"), Enter.
Keys '{TAB}{TAB}' 300; Keys '{ENTER}' 3500
Shot "$P-3-page"
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'blocked|redirected|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
