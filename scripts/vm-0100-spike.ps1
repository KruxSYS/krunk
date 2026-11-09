param([string]$Mode = 'aura', [string]$App = 'C:\pipeline\krunk\app-spike')
# Krunk 0.10.0 (P3): the spike's required cases for a hidden background tab, with the spike engine
# in KRUNK_HIDDEN_MODE=$Mode (aura | direct | off). Six tabs from a saved session:
#   1 spike.html state   text typed into a field, the page scrolled to 1400
#   2 spike.html audio   playing with sound ('p')
#   3 spike.html rtc     a local WebRTC call (canvas stream through two peer connections)
#   4 spike.html notify  a notification 8 s after 'n' (the tab is hidden by then; allowed up front)
#   5 spike.html dl      a 20-MB download over 20 s ('d'), the tab hidden meanwhile
#   6 busy.html          a busy page that should be throttled
# Then: 60 s with tab 1 in front; every tab shown again (a shot each: not blank, state kept);
# DevTools on tab 1 across a switch; print preview; minimise/restore, covered by another window
# and moved/resized, with tab 1 in front (it must stay visible, tab 6 hidden).
# Every page reports to report-serve.py (/m); the reports are copied to the shots.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "spike-$Mode"
$K = 'C:\pipeline\krunk'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'High'
Get-Process Krunk, notepad -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
$log = "$K\reports-0100.log"
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'report-[s]erve\.py' })) {
  Start-Process python -WindowStyle Hidden -ArgumentList "$K\report-serve.py", "$K\www\8094", '8097', $log
  Start-Sleep 2
}
$mark = (Get-Item $log -ErrorAction SilentlyContinue).Length; if (-not $mark) { $mark = 0 }
$stun = (Invoke-WebRequest -UseBasicParsing 'http://localhost:8097/stun-host').Content.Trim()
$T = "sp$Mode"
$base = 'http://localhost:8097/spike.html'
$urls = @("$base`?case=state&tag=$T-state", "$base`?case=audio&tag=$T-audio", "$base`?case=rtc&tag=$T-rtc&stun=$stun",
  "$base`?case=notify&tag=$T-notify", "$base`?case=dl&tag=$T-dl", "http://localhost:8097/busy.html?tag=$T-busy")
$profile = "$K\p0100-spike"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
$tabs = @($urls | ForEach-Object { @{ url = $_; title = $_ } })
$session = @{ version = 2; tabs = $tabs; active = 0; windows = @(@{ tabs = $tabs; active = 0; maximized = $false }); focused = 0 }
[IO.File]::WriteAllText("$K\session-0100.json", ($session | ConvertTo-Json -Depth 6 -Compress), (New-Object Text.UTF8Encoding($false)))
python $K\setkv.py $profile "@$K\session-0100.json" session | Out-Null
python $K\setkv.py $profile '{\"checkUpdatesAutomatically\": false, \"startup\": \"restoreSession\"}' | Out-Null
python -c "import sqlite3,sys,time; d=sqlite3.connect(sys.argv[1]); d.execute('INSERT OR REPLACE INTO site_permissions VALUES (?,?,?,?)', ('http://localhost:8097','notifications',1,int(time.time()*1000))); d.commit()" "$profile\Krunk\browser.db"
$dl = "$env:USERPROFILE\Downloads"
Remove-Item "$dl\spike-slow*.bin" -ErrorAction SilentlyContinue
$env:LOCALAPPDATA = $profile
$env:KRUNK_HIDDEN_MODE = $Mode
Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function Mark([string]$what) { Invoke-WebRequest -UseBasicParsing "http://localhost:8097/m?$([uri]::EscapeDataString("$T-harness $what"))" | Out-Null; Log "[$P] $what" }
$build = (Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion
Log "[$P] krunk $build, KRUNK_HIDDEN_MODE=$Mode, libcef $((Get-FileHash "$App\libcef.dll").Hash.Substring(0, 12))"

# Load every tab and start each case.
Start-Sleep 2
Click (X 1110) (Y 132) 400; SafeKeys 'krunk-state-123' 500                 # tab 1: the field (top right, under the bookmarks bar)
SafeKeys '^2' 2500; Click (X 450) (Y 500) 300; SafeKeys 'p' 1500            # audio
SafeKeys '^3' 4000                                                         # rtc
SafeKeys '^4' 2500; Click (X 450) (Y 500) 300; SafeKeys 'n' 300             # notify in 8 s
SafeKeys '^5' 2500; Click (X 450) (Y 500) 300; SafeKeys 'd' 300             # 20-MB download
SafeKeys '^6' 3000                                                         # busy
SafeKeys '^1' 1000
Mark 'tab 1 in front, others hidden for 60 s'
Shot "$P-1-front"
Start-Sleep 60
foreach ($n in 2, 3, 4, 5, 6, 1) {
  Mark "show tab $n"
  SafeKeys "^$n" 2500
  Shot "$P-2-tab$n"
  Start-Sleep 2
}
# DevTools on tab 1 across a switch.
Mark 'devtools open on tab 1'
SafeKeys '{F12}' 4000
Shot "$P-3-devtools"
SafeKeys '^2' 15000
Mark 'back to tab 1 with devtools'
SafeKeys '^1' 3000
Shot "$P-4-devtools-back"
SafeKeys '{F12}' 1500
# Print preview of the woken tab.
Mark 'print preview'
SafeKeys '^p' 5000
Shot "$P-5-print"
$pd = [U]::FindWindow('#32770', 'Print')
if ($pd -ne [IntPtr]::Zero) { Focus $pd | Out-Null; Keys '{ESC}' 1500 }
Log "[$P] print dialog $(if ($pd -ne [IntPtr]::Zero) { 'seen' } else { 'not found' }), still open: $([U]::FindWindow('#32770', 'Print') -ne [IntPtr]::Zero)"
Focus $A | Out-Null
# Window states with tab 1 in front.
Mark 'minimised'
[U]::ShowWindow($A, 6) | Out-Null; Start-Sleep 11
Mark 'restored'
[U]::ShowWindow($A, 9) | Out-Null; Start-Sleep 11
Focus $A | Out-Null
Shot "$P-6-restored"
Mark 'covered by notepad'
$np = Start-Process notepad -PassThru; Start-Sleep 2
$nh = $np.MainWindowHandle; if ($nh -eq 0) { Start-Sleep 2; $nh = (Get-Process -Id $np.Id).MainWindowHandle }
[U]::ShowWindow($nh, 3) | Out-Null; Start-Sleep 11
Stop-Process -Id $np.Id -Force
Mark 'uncovered'
Focus $A | Out-Null; Start-Sleep 11
Mark 'moved to the right half'
[U]::MoveWindow($A, 640, 0, 640, 760, $true) | Out-Null; Start-Sleep 6
Mark 'maximised'
[U]::ShowWindow($A, 3) | Out-Null; Start-Sleep 6
Shot "$P-7-maximised"
Mark 'moved back'
[U]::ShowWindow($A, 9) | Out-Null; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 11
$f = Get-Item "$dl\spike-slow.bin" -ErrorAction SilentlyContinue
Log "[$P] download: $(if ($f) { "$($f.Length) bytes" } else { 'missing' }) ($((Get-ChildItem $dl -Filter 'spike-slow*' | ForEach-Object { "$($_.Name)=$($_.Length)" }) -join ', '))"
Mark 'end'
Start-Sleep 1
$fs = [System.IO.File]::Open($log, 'Open', 'Read', 'ReadWrite'); $fs.Seek($mark, 'Begin') | Out-Null
$text = (New-Object System.IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
$lines = $text -split "`n" | Where-Object { $_ -match " $T-" } | ForEach-Object { $_.TrimEnd() }
$lines | Set-Content -Encoding utf8 "$Shots\$P-reports.txt"
Log "[$P] page reports: $($lines.Count) lines"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
