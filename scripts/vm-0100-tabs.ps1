param([string]$Browser = 'krunk', [int]$N = 10, [string]$Tag = 'k10', [string]$App = 'C:\pipeline\krunk\app', [int]$Real = 0, [string]$Fg = 'latency', [string]$Mode = '', [string]$Bg = '')
# Krunk 0.10.0 (P1): many busy tabs. Opens the foreground probe (latency.html) and N-1 busy
# background tabs (busy.html, each with a cross-site busy frame; with -Real the first ones are real
# sites) in Krunk, Chrome or Edge, lets them settle, then measures for 30 s:
#   - CPU per process type (browser, GPU, renderer, utility) from the processes' CPU time, and the
#     whole machine's CPU load;
#   - what background tabs get (frames/s, timer ticks/s, visibility) and what the active tab gets
#     (frames/s, timer lateness), from the pages' own reports in www\access.log;
#   - input latency (key → painted frame) in the active tab, and tab-switch latency (key → the
#     switched-to tab's focus event).
# Krunk gets its tabs as a saved session (restored tabs load when first shown, so one pass of
# Ctrl+Tab loads them all); Chrome and Edge get the URLs on their command line.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "tabs-$Tag"
$K = 'C:\pipeline\krunk'
# A saturated VM (30 unthrottled tabs) starved this script and sshd for 20 min in P1's first run:
# both run above the browsers.
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'High'
Get-Process sshd -ErrorAction SilentlyContinue | ForEach-Object { try { $_.PriorityClass = 'AboveNormal' } catch {} }
$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
Get-Process Krunk, chrome, msedge -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
# The pages and their reports: report-serve.py on 8097 (scripts/perf; threaded, its own log).
$log = "$K\reports-0100.log"
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'report-[s]erve\.py' })) {
  Start-Process python -WindowStyle Hidden -ArgumentList "$K\report-serve.py", "$K\www\8094", '8097', $log
  Start-Sleep 2
}
Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'report-[s]erve\.py' } |
  ForEach-Object { try { (Get-Process -Id $_.ProcessId).PriorityClass = 'High' } catch {} }
$real = @('https://www.cnn.com/', 'https://www.theverge.com/', 'https://www.youtube.com/', 'https://www.reddit.com/', 'https://www.bbc.com/news')
$urls = @("http://localhost:8097/latency.html?tag=$Tag-fg")
# -Fg speedometer: the active tab runs Speedometer 3.1 (3 iterations; speedometer\sp-serve.py on 8096).
if ($Fg -eq 'speedometer') {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'sp-[s]erve\.py' })) {
    Start-Process python -WindowStyle Hidden -ArgumentList "$K\speedometer\sp-serve.py", '8096'
    Start-Sleep 2
  }
  Remove-Item "$K\speedometer\results\latest.json", "$K\speedometer\results\errors.txt" -ErrorAction SilentlyContinue
  $urls = @('http://127.0.0.1:8096/index.html?startAutomatically=true&iterationCount=3')
}
for ($i = 1; $i -lt $N; $i++) {
  if ($i -le $Real) { $urls += $real[($i - 1) % $real.Count] } else { $urls += "http://localhost:8097/busy.html?tag=$Tag-bg$i$(if ($i % 3 -eq 0) { '&video=1' })$Bg" }
}
$mark = (Get-Item $log -ErrorAction SilentlyContinue).Length
if (-not $mark) { $mark = 0 }
$profile = "$K\p0100-$Browser"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
$t0 = Get-Date
if ($Browser -eq 'krunk') {
  Copy-Item -Recurse "$K\sp-tpl" $profile
  $tabs = @($urls | ForEach-Object { @{ url = $_; title = $_ } })
  $session = @{ version = 2; tabs = $tabs; active = 0; windows = @(@{ tabs = $tabs; active = 0; maximized = $true }); focused = 0 }
  # (PowerShell 5's -Encoding utf8 writes a BOM, which Python's json refuses.)
  [IO.File]::WriteAllText("$K\session-0100.json", ($session | ConvertTo-Json -Depth 6 -Compress), (New-Object Text.UTF8Encoding($false)))
  $r = python $K\setkv.py $profile "@$K\session-0100.json" session 2>&1
  Log "[$P] session written: $(([string]$r).Length) chars of output, tabs in it: $(([regex]::Matches([string]$r, '"url"')).Count)"
  Log "[$P] settings: $(python $K\setkv.py $profile '{\"checkUpdatesAutomatically\": false, \"startup\": \"restoreSession\"}')"
  $env:LOCALAPPDATA = $profile
  # P3 spike engines read KRUNK_HIDDEN_MODE (aura | direct | off); release engines ignore it.
  $env:KRUNK_HIDDEN_MODE = $Mode
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $A = MainWindow
  [U]::ShowWindow($A, 3) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
  # One pass through every tab loads the restored ones, then back to the probe.
  for ($i = 1; $i -lt $N; $i++) { SafeKeys '^{TAB}' 350 }
  SafeKeys '^1' 800
  $build = (Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion
} else {
  $exe = if ($Browser -eq 'edge') { 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe' } else { 'C:\Program Files\Google\Chrome\Application\chrome.exe' }
  $argv = @("--user-data-dir=$profile", '--no-first-run', '--no-default-browser-check', '--disable-search-engine-choice-screen', '--start-maximized') + $urls
  Start-Process $exe -ArgumentList $argv
  Start-Sleep 8
  $A = (Get-Process $(if ($Browser -eq 'edge') { 'msedge' } else { 'chrome' }) | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowHandle
  Focus $A | Out-Null
  Keys '^1' 800
  $build = (Get-Item $exe).VersionInfo.ProductVersion
}
if ($Mode) { $build = "$build/$Mode" }
Log "[$P] ${Browser} ${build}: $N tabs ($Real real sites) open after $([int]((Get-Date) - $t0).TotalSeconds) s"
if ($Fg -eq 'speedometer') {
  # Restart the benchmark now that every tab is open (the first load started it too early).
  if ($Browser -eq 'krunk') { SafeKeys '{F5}' 500 } else { Keys '{F5}' 500 }
  Remove-Item "$K\speedometer\results\latest.json" -ErrorAction SilentlyContinue
  for ($i = 0; $i -lt 150 -and -not (Test-Path "$K\speedometer\results\latest.json"); $i++) { Start-Sleep 4 }
  $score = 'none'
  if (Test-Path "$K\speedometer\results\latest.json") {
    Copy-Item "$K\speedometer\results\latest.json" "$Shots\$P-speedometer.json"
    $score = '{0:N2}' -f (Get-Content "$K\speedometer\results\latest.json" -Raw | ConvertFrom-Json).Score.mean
  }
  Log "[$P] RESULT speedometer score=$score after $([int]((Get-Date) - $t0).TotalSeconds) s"
  Shot "$P-speedometer"
  Get-Process Krunk, chrome, msedge -ErrorAction SilentlyContinue | Stop-Process -Force
  Log "[$P] done"
  exit
}
Start-Sleep 20
Shot "$P-1-open"

# Process types once (CIM is slow on a loaded machine); snapshots by id.
$name = if ($Browser -eq 'krunk') { 'Krunk.exe' } elseif ($Browser -eq 'edge') { 'msedge.exe' } else { 'chrome.exe' }
$types = @{}
Get-CimInstance Win32_Process -Filter "Name='$name'" | ForEach-Object {
  $m = [regex]::Match([string]$_.CommandLine, '--type=([a-z-]+)')
  $types[[int]$_.ProcessId] = if ($m.Success) { $m.Groups[1].Value } else { 'browser' }
}
function Procs {
  $snap = @{}
  foreach ($p in Get-Process -Id @($types.Keys) -ErrorAction SilentlyContinue) { $snap[$p.Id] = @($types[$p.Id], $p.TotalProcessorTime.TotalSeconds, $p.WorkingSet64) }
  $snap
}
# 30 s measurement window: process CPU, machine CPU, the pages' own reports.
$w0 = Get-Date
$s0 = Procs
$machine = (Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 2 -MaxSamples 15).CounterSamples | Measure-Object CookedValue -Average
$s1 = Procs
$w1 = Get-Date
$secs = ($w1 - $w0).TotalSeconds
$byType = @{}; $ws = 0; $count = @{}
foreach ($id in $s1.Keys) {
  if (-not $s0.ContainsKey($id)) { continue }
  $t = $s1[$id][0]; $d = $s1[$id][1] - $s0[$id][1]
  $byType[$t] = [double]$byType[$t] + $d
  $count[$t] = [int]$count[$t] + 1
  $ws += $s1[$id][2]
}
$total = 0; $parts = @()
foreach ($t in ($byType.Keys | Sort-Object)) {
  $pct = 100 * $byType[$t] / ($secs * $cores); $total += $pct
  $parts += ('{0}={1:N1}% ({2})' -f $t, $pct, $count[$t])
}
Log ("[$P] RESULT cpu total={0:N1}% of {1} cores; {2}; machine={3:N1}%; working set={4:N0} MB; processes={5}; window={6:N0} s" -f $total, $cores, ($parts -join ' '), $machine.Average, ($ws / 1MB), $s1.Count, $secs)

# Input latency: 20 keys into the probe page, each one's send time logged.
$keys = @()
for ($i = 0; $i -lt 20; $i++) { $keys += [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); Keys 'a' 150 }
Log "[$P] keys $($keys -join ' ')"
# Tab-switch latency: to tabs 2..5 and back, each key's time logged first.
$switches = @()
foreach ($n in 2, 3, 4, 5) {
  if ($n -gt $N) { break }
  $switches += "$n@$([DateTimeOffset]::Now.ToUnixTimeMilliseconds())"
  if ($Browser -eq 'krunk') { SafeKeys "^$n" 1500 } else { Keys "^$n" 1500 }
}
if ($Browser -eq 'krunk') { SafeKeys '^1' 800 } else { Keys '^1' 800 }
Log "[$P] switches $($switches -join ' ')"
Start-Sleep 12   # late reports (a saturated machine delays them by seconds)
Shot "$P-2-end"

# The pages' reports from this run (from the log offset taken before the start).
$fs = [System.IO.File]::Open($log, 'Open', 'Read', 'ReadWrite'); $fs.Seek($mark, 'Begin') | Out-Null
$text = (New-Object System.IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
$lines = $text -split "`n" | Where-Object { $_ -match " $Tag-" } | ForEach-Object { $_.TrimEnd() }
$lines | Set-Content -Encoding utf8 "$Shots\$P-reports.txt"
Log "[$P] page reports: $($lines.Count) lines"
if ($Browser -eq 'krunk') { Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue }
Get-Process Krunk, chrome, msedge -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] window $(Get-Date $w0 -Format o) .. $(Get-Date $w1 -Format o)"
Log "[$P] done"
