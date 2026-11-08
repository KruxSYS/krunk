$Theme = 'dark'
# Krunk 0.7.0 verification (R7): no white flash when switching onto discarded tabs, 21 times.
# 21 tabs of a dark page sit in the background past the discard time (--sleep-test-seconds=10:
# discarded after 40 s) while a new tab page is active; tools\framecap.exe then records the
# content area while Ctrl+Tab activates each discarded tab in turn (each one reloads).
# scripts/frames/analyze.py counts the flash frames on the build host. Fresh profile (copy of
# sp-tpl) with the theme set.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "discard-$Theme"
$K = 'C:\pipeline\krunk'
$N = 21
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p070d"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$profile\Krunk\browser.db"
@{ theme = $Theme; tabDiscardEnabled = $true } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app" -ArgumentList '--sleep-test-seconds=10'
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
$top = [int]($c.Top + 112 * $c.Scale)
$rect = "$($c.Left + 4),$top,$($c.Right - $c.Left - 8),$($c.Bottom - $top - 4)"
Log "[$P] content rect $rect"

# $N dark tabs, then a new tab page as the active tab.
SafeGo 'http://localhost:8094/dark.html?t=1' 1500
for ($i = 2; $i -le $N; $i++) { SafeKeys '^t' 900; SafeGo "http://localhost:8094/dark.html?t=$i" 1200 }
SafeKeys '^t' 2000
Shot "$P-1-tabs"
Log "[$P] waiting for $N background tabs to be discarded"
$gone = 0
for ($i = 0; $i -lt 36; $i++) {
  Start-Sleep 5
  $gone = @(Select-String -Path "$profile\Krunk\krunk.log" -Pattern ': discarded').Count
  if ($gone -ge $N) { break }
}
Log "[$P] discarded tabs: $gone"

$cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '4', '--seconds', 30, '--out', "$Shots\$P-cap" -WindowStyle Hidden -PassThru
Start-Sleep -Milliseconds 800
# Ctrl+Tab from the last tab wraps to tab 1: every switch lands on a discarded tab.
for ($i = 0; $i -lt $N; $i++) { SafeKeys '^{TAB}' 1200 }
$cap.WaitForExit(40000) | Out-Null
Shot "$P-2-after-switches"
Log "[$P] switches onto discarded tabs: $N"
Log "[$P] capture done: $(Get-Content "$Shots\$P-cap.json")"
$re = @(Select-String -Path "$profile\Krunk\krunk.log" -Pattern 'reload|restor' | Select-Object -Last 3)
Log "[$P] log: $($re -join ' / ')"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
