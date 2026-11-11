# Krunk 0.15.0: the one targeted VM pass (plan-0.15.0.md M4). The release build in
# C:\pipeline\krunk\app, fresh profiles per phase, the window at 0,0 1280x760 (96 DPI unless a
# phase changes it). Test pages: www\8094\c1..c5.html (scripts/vm-www/8094, one colour each); authserve.py on 8110 (scripts/vm-0150-authserve.py)
# (Basic, realm "Krunk test", krunk:test); the mock AI provider on 127.0.0.1:18111 through
# `ssh -R` from the build host. Each phase fits one KrunkTask run (10 min):
#   tabs      framecaps of new tab and close-active-tab (dark theme), the hover card, the
#             tab-scoped sign-in dialog (switch, away and back, badge, Cancel)
#   ai        the AI panel light → dark live (the agent changes the theme), max-tokens stop,
#             cut-off tool call + script saved in parts, new tab background from a URL
#   styles    one gradient and one glass style (light and dark); the NTP with pinned tiles
#   tiles     a pinned tile hovered at 100 % and 125 % (its centre on screen: $TileX/$TileY at
#             100 %, $TileX125/$TileY125 at 125 %; the grid is centred, so read them from shots)
#   dl        krunk://downloads with 2,000 seeded entries (top, scrolled, end; #perf overlay);
#             test.test → search; "why do" suggestions
#   sessions  idle CPU over 60 s with session logs off, then on; a short session; the list and
#             the detail on krunk://sessions
param([string]$Phase = 'tabs', [int]$TileX = 0, [int]$TileY = 0, [int]$TileX125 = 0, [int]$TileY125 = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$W = 'http://127.0.0.1:8094'
$Py = 'C:\Program Files\Python312\python.exe'
$P = "v0150-$Phase"
Remove-Item "$Shots\$P-*" -Force -ErrorAction SilentlyContinue

function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-Process Krunk -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
  Start-Sleep 1
}
function StartKrunk([string]$prof) {
  $env:LOCALAPPDATA = $prof
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $script:A)) { Log "[$P] could not focus"; throw 'no focus' }
  $script:c = ClientRect $script:A
}
# A new profile: one start creates the database (storage V10), then the settings patch.
function Fresh([string]$prof, [string]$patch) {
  StopKrunk
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $prof | Out-Null
  StartKrunk $prof
  StopKrunk
  SetKv $prof $patch
}
function SetKv([string]$prof, [string]$patch) {
  [IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
  Log "[$P] setkv: $(& $Py "$K\setkv.py" $prof "@$prof\patch.json" 2>&1)"
}
function Page([string]$url, [int]$wait = 2500) { Focus (MainWindow) | Out-Null; SafeGo $url $wait }
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function KLog([string]$prof, [string]$pattern) {
  (Get-Content "$prof\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join "`n"
}
function Capture([string]$name, [int]$seconds, [string]$rect) {
  Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '4', '--seconds', $seconds, '--out', "$Shots\$name" -WindowStyle Hidden -PassThru
}
# CPU seconds of all Krunk processes over $sec seconds (processes that start or end meanwhile
# count from when they are seen).
function CpuOver([int]$sec) {
  $a = @{}; Get-Process Krunk | ForEach-Object { $a[$_.Id] = $_.CPU }
  Start-Sleep $sec
  $sum = 0.0
  Get-Process Krunk | ForEach-Object { $b = $a[$_.Id]; if ($null -eq $b) { $b = 0 }; $sum += ($_.CPU - $b) }
  [Math]::Round($sum, 3)
}
$base = '"bookmarksBar": "never", "checkUpdatesAutomatically": false'

if ($Phase -eq 'tabs') {
  if (-not (Get-NetTCPConnection -LocalPort 8110 -State Listen -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath $Py -ArgumentList "$K\authserve.py", '8110' -WindowStyle Hidden
    Start-Sleep 2
  }
  $prof = "$K\p0150t"
  Fresh $prof "{ $base, `"theme`": `"dark`" }"
  StartKrunk $prof
  Page "$W/c1.html"
  # The page area: below the tab strip and toolbar (76 DIP), inside the window border.
  $top = Y 80
  $rect = "$($c.Left + 4),$top,$($c.Right - $c.Left - 8),$($c.Bottom - $top - 4)"
  Log "[$P] content rect $rect scale $($c.Scale)"
  # (1) New tabs from a loaded page: 5 × Ctrl+T.
  $cap = Capture "$P-cap-newtab" 12 $rect
  Start-Sleep -Milliseconds 800
  for ($i = 0; $i -lt 5; $i++) { SafeKeys '^t' 1700 }
  $cap.WaitForExit(30000) | Out-Null
  Shot "$P-01-after-newtabs"
  for ($i = 0; $i -lt 5; $i++) { SafeKeys '^w' 700 }
  # (2) Closing the active tab, four times, each time onto a loaded page.
  foreach ($n in 2..5) { SafeKeys '^t' 900; SafeGo "$W/c$n.html" 1800 }
  Shot "$P-02-five-pages"
  $cap = Capture "$P-cap-close" 11 $rect
  Start-Sleep -Milliseconds 800
  for ($i = 0; $i -lt 4; $i++) { SafeKeys '^w' 1800 }
  $cap.WaitForExit(30000) | Out-Null
  Shot "$P-03-after-closes"
  # (3) The hover card on a background tab (c1 was left for a new tab: its picture exists).
  SafeKeys '^t' 900; SafeGo "$W/c2.html" 2500
  $t0 = TabPoint $A 0 2
  MoveTo ($t0[0] - 30) ($t0[1] + 300); Start-Sleep -Milliseconds 300
  MoveTo $t0[0] $t0[1]; Start-Sleep -Milliseconds 1400
  Shot "$P-04-hover-card"
  MoveTo 640 450; Start-Sleep -Milliseconds 600
  # (4) Sign-in dialog: shown, tab switch (hidden, badge), back, away to Notepad and back, Cancel.
  SafeKeys '^t' 900; SafeGo 'http://127.0.0.1:8110/' 3000
  Shot "$P-05-auth-dialog"
  $t = TabPoint $A 0 3; Click $t[0] $t[1] 1500
  Shot "$P-06-auth-switched-badge"
  $t = TabPoint $A 2 3; Click $t[0] $t[1] 1500
  Shot "$P-07-auth-back"
  # Notepad's launcher exits at once: the window's process is found by its start time.
  $t0 = Get-Date
  Start-Process notepad
  Start-Sleep 2
  Shot "$P-08-auth-away"
  Focus (MainWindow) | Out-Null; Start-Sleep -Milliseconds 1500
  Shot "$P-09-auth-away-and-back"
  Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t0 } | Stop-Process -Force
  Focus (MainWindow) | Out-Null; Start-Sleep -Milliseconds 1000
  Keys '{ESC}' 1500
  Shot "$P-10-auth-cancelled"
  Log "[$P] auth log:`n$(KLog $prof 'http auth')"
  Log "[$P] hover log:`n$(KLog $prof 'hover')"
  StopKrunk
  Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
}

if ($Phase -eq 'ai') {
  $prof = "$K\p0150a"
  Fresh $prof "{ $base, `"theme`": `"light`", `"ai`": {`"enabled`": true, `"providers`": [{`"id`": `"0a1b2c3d`", `"kind`": `"custom`", `"name`": `"Mock`", `"baseUrl`": `"http://127.0.0.1:18111/v1`"}], `"activeProvider`": `"0a1b2c3d`", `"activeModel`": `"mock-agent`"} }"
  StartKrunk $prof
  Page "$W/c1.html"
  SafeKeys '^+ ' 1800
  Shot "$P-01-panel-light"
  function Ask([string]$text, [int]$wait) {
    Focus (MainWindow) | Out-Null
    Click ($c.Right - 190) ($c.Bottom - 26) 300
    SafeKeys $text 150
    SafeKeys '{ENTER}' $wait
  }
  Ask 'switch to dark theme' 5000
  Shot "$P-02-panel-dark-live"
  Ask 'hit the output limit' 9000
  Shot "$P-03-max-tokens"
  Ask 'cut off script' 16000
  Shot "$P-04-cut-off-and-parts"
  Page 'https://example.com/' 4000
  Shot "$P-05-script-runs"
  Ask 'set the new tab background from https://upload.wikimedia.org/wikipedia/commons/thumb/3/3f/Fronalpstock_big.jpg/1280px-Fronalpstock_big.jpg' 15000
  Shot "$P-06-ntp-url-step"
  SafeKeys '^t' 3000
  Shot "$P-07-ntp-background"
  Log "[$P] ntp files: $((Get-ChildItem "$prof\Krunk" -Filter 'ntp-background-*' | ForEach-Object { "$($_.Name) $($_.Length)" }) -join ', ')"
  Log "[$P] agent log:`n$(KLog $prof 'agent')"
  StopKrunk
  Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
}

if ($Phase -eq 'styles') {
  $prof = "$K\p0150s"
  $grad = '"appearance": {"tabStyle":"gradient","toolbarStyle":"gradient","gradientFrom":"#0ea5e9","gradientVia":"","gradientTo":"#14b8a6","gradientAngle":135,"preset":"","tabShape":"pill","tabRadius":8,"activeHighlight":"outline","accentGlow":false,"roundedTabs":true}'
  $glass = '"appearance": {"tabStyle":"glass","toolbarStyle":"glass","gradientFrom":"","gradientVia":"","gradientTo":"","gradientAngle":90,"preset":"glass","tabShape":"rounded","tabRadius":8,"activeHighlight":"underline","accentGlow":true,"roundedTabs":true}'
  Fresh $prof "{ $base, `"theme`": `"light`", $grad }"
  # Pinned tiles for the new tab page (tiles.state 1 = pinned).
  & $Py -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); [d.execute('INSERT OR REPLACE INTO tiles(url,title,state,position) VALUES (?,?,1,?)', (f'http://127.0.0.1:8094/c{i}.html', t, i)) for i, t in ((1,'Blue page'),(2,'Green page'),(3,'Pink page'))]; d.commit()" "$prof\Krunk\browser.db"
  foreach ($look in @(@('gradient', $grad), @('glass', $glass))) {
    foreach ($theme in 'light', 'dark') {
      StopKrunk
      SetKv $prof "{ `"theme`": `"$theme`", $($look[1]) }"
      StartKrunk $prof
      Page "$W/c1.html" 1500
      SafeKeys '^t' 1200; SafeGo 'https://example.com/' 2500
      SafeKeys '^t' 2500
      Shot "$P-$($look[0])-$theme"
    }
  }
  StopKrunk
}

if ($Phase -eq 'tiles') {
  $prof = "$K\p0150s"
  foreach ($pct in 100, 125) {
    StopKrunk
    & powershell -NoProfile -File "$K\vm-dpi.ps1" -Percent $pct | ForEach-Object { Log "[$P] dpi: $_" }
    Start-Sleep 3
    foreach ($theme in 'light', 'dark') {
      StopKrunk
      SetKv $prof "{ `"theme`": `"$theme`", `"appearance`": {`"tabStyle`":`"solid`",`"toolbarStyle`":`"solid`",`"preset`":`"`"} }"
      StartKrunk $prof
      SafeKeys '^t' 2500
      if ($pct -eq 100) { $tx = $TileX; $ty = $TileY } else { $tx = $TileX125; $ty = $TileY125 }
      Log "[$P] $pct %: scale $($c.Scale), tile at $tx,$ty"
      MoveTo 640 700; Start-Sleep -Milliseconds 400
      Shot "$P-$pct-$theme-idle"
      MoveTo $tx $ty; Start-Sleep -Milliseconds 900
      Shot "$P-$pct-$theme-hover"
    }
  }
  StopKrunk
  & powershell -NoProfile -File "$K\vm-dpi.ps1" -Percent 100 | ForEach-Object { Log "[$P] dpi: $_" }
}

if ($Phase -eq 'dl') {
  $prof = "$K\p0150d"
  Fresh $prof "{ $base }"
  Log "[$P] seed: $(& $Py "$K\seed-downloads.py" "$prof\Krunk\browser.db" 2000 2>&1)"
  StartKrunk $prof
  Page 'krunk://downloads#perf' 3000
  Shot "$P-01-top"
  MoveTo 640 400
  for ($i = 0; $i -lt 12; $i++) { Wheel 640 400 5; Start-Sleep -Milliseconds 120 }
  Start-Sleep -Milliseconds 800
  Shot "$P-02-scrolled"
  Click 1100 300 200
  SafeKeys '{END}' 2000
  Shot "$P-03-end"
  SafeKeys '{HOME}' 1500
  # The address bar: a made-up TLD is searched; suggestions for "why do".
  SafeKeys '^t' 1200
  SafeGo 'test.test' 4000
  SafeKeys '^l' 300; SafeKeys '^c' 300
  Log "[$P] test.test opened: $(Get-Clipboard)"
  Keys '{ESC}' 300
  Shot "$P-04-test-test"
  SafeKeys '^l' 300; SafeKeys 'why do' 100
  Start-Sleep -Milliseconds 2500
  Shot "$P-05-why-do"
  Keys '{ESC}' 300; Keys '{ESC}' 300
  StopKrunk
  Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
}

if ($Phase -eq 'sessions') {
  $prof = "$K\p0150c"
  Fresh $prof "{ $base }"
  foreach ($on in $false, $true) {
    StopKrunk
    SetKv $prof "{ `"sessionLog`": {`"enabled`": $($on.ToString().ToLower()), `"retentionDays`": 30} }"
    StartKrunk $prof
    Page "$W/c1.html" 3000
    Start-Sleep 40
    $r = @(CpuOver 60; CpuOver 60)
    Log "[$P] idle CPU s per 60 s, session logs $(if ($on) { 'on' } else { 'off' }): $($r -join ', ')"
  }
  # Logging is on: a short session (three pages in two tabs, switching between them).
  Page "$W/c2.html" 6000
  SafeKeys '^t' 800; SafeGo "$W/c3.html" 8000
  SafeKeys '^1' 8000; SafeKeys '^2' 8000; SafeKeys '^1' 6000
  Page 'krunk://sessions' 3000
  Shot "$P-01-list"
  Click (X 250) (Y 184) 2500
  Shot "$P-02-detail"
  Wheel 640 500 10; Start-Sleep -Milliseconds 800
  Shot "$P-03-detail-lower"
  Log "[$P] session log:`n$(KLog $prof 'session')"
  StopKrunk
  Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
}
# Idle CPU with session logs off and on, from the performance counters (Process.CPU could not
# read the sandboxed engine processes from the task: it stayed at 0).
if ($Phase -eq 'cpu') {
  $prof = "$K\p0150u"
  Fresh $prof "{ $base }"
  $cores = [Environment]::ProcessorCount
  foreach ($on in $false, $true, $false, $true) {
    StopKrunk
    SetKv $prof "{ `"sessionLog`": {`"enabled`": $($on.ToString().ToLower()), `"retentionDays`": 30} }"
    StartKrunk $prof
    Page "$W/c1.html" 3000
    Start-Sleep 30
    $samples = Get-Counter '\Process(krunk*)\% Processor Time' -SampleInterval 1 -MaxSamples 60 -ErrorAction SilentlyContinue
    $tot = @($samples | ForEach-Object { ($_.CounterSamples | Measure-Object CookedValue -Sum).Sum })
    $avg = ($tot | Measure-Object -Average).Average
    Log ("[$P] session logs {0}: Krunk processes {1}, 60 samples, average {2:N3} % of one core ({3:N4} % of all {4} cores), CPU-seconds per 60 s {5:N3}" -f $(if ($on) { 'on ' } else { 'off' }), @(Get-Process Krunk).Count, $avg, ($avg / $cores), $cores, ($avg * 60 / 100))
  }
  StopKrunk
}
Log "[$P] done"
