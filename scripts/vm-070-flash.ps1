$Theme = 'dark'
# Krunk 0.7.0 M2 (R7): no white flash on tab switches. With the theme $Theme, tools\framecap.exe
# records every frame of the content area while Ctrl+Tab switches 24 times between a dark page and
# the new tab page, then between the new tab page and a tab still loading (an address that never
# answers), then once onto a discarded tab (reloaded on activation). scripts/frames/analyze.py
# counts the flash frames on the build host. Fresh profile (copy of sp-tpl) with the theme set.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "flash-$Theme"
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p070f"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$profile\Krunk\browser.db"
@{ theme = $Theme; tabDiscardEnabled = $true } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
$env:LOCALAPPDATA = $profile
# --sleep-test-seconds=10: a background tab sleeps after 10 s and is discarded after 40 s.
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app" -ArgumentList '--sleep-test-seconds=10'
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# The content area: below the toolbar and the bookmarks bar.
$top = [int]($c.Top + 112 * $c.Scale)
$rect = "$($c.Left + 4),$top,$($c.Right - $c.Left - 8),$($c.Bottom - $top - 4)"
Log "[$P] content rect $rect"

function Capture([string]$name, [int]$seconds) {
  Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '4', '--seconds', $seconds, '--out', "$Shots\$name" -WindowStyle Hidden -PassThru
}

# (1) A dark page and the new tab page, 24 switches.
SafeGo 'http://localhost:8094/dark.html' 2500
SafeKeys '^t' 2500
Shot "$P-1-ntp"
$cap = Capture "$P-cap1" 22
Start-Sleep -Milliseconds 800
for ($i = 0; $i -lt 24; $i++) { SafeKeys '^{TAB}' 700 }
$cap.WaitForExit(30000) | Out-Null
Shot "$P-2-after-switches"
Log "[$P] capture 1 done: $(Get-Content "$Shots\$P-cap1.json")"

# (2) The dark page and a new tab that is still loading (opened from a link to an address that
# never answers: a new view with nothing painted yet).
SafeKeys '^1' 1500
Click (X 150) (Y 236) 2500
Shot "$P-3-loading"
$cap = Capture "$P-cap2" 14
Start-Sleep -Milliseconds 800
for ($i = 0; $i -lt 16; $i++) { SafeKeys '^1' 350; SafeKeys '^2' 350 }
$cap.WaitForExit(30000) | Out-Null
Log "[$P] capture 2 done: $(Get-Content "$Shots\$P-cap2.json")"

# (3) A discarded tab: the dark page sits in the background past the discard time (40 s here).
SafeKeys '^3' 1000
Log "[$P] waiting for the dark page (tab 1) to be discarded"
for ($i = 0; $i -lt 30; $i++) {
  Start-Sleep 10
  if (Select-String -Path "$profile\Krunk\krunk.log" -Pattern 'discarded' -Quiet) { break }
}
$disc = Get-Content "$profile\Krunk\krunk.log" | Select-String 'discard|unload' | Select-Object -Last 3
Log "[$P] log: $($disc -join ' / ')"
$cap = Capture "$P-cap3" 8
Start-Sleep -Milliseconds 800
SafeKeys '^1' 4000
$cap.WaitForExit(20000) | Out-Null
Shot "$P-4-discarded-back"
Log "[$P] capture 3 done: $(Get-Content "$Shots\$P-cap3.json")"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
