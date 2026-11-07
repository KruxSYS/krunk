# Krunk 0.7.0 M6 (R9-R11): downloads in parts that rebalance, survive a restart and fall back
# when the file changes; the flight and the bars measured frame by frame (tools\framecap).
#   steal   64 MiB over 4 connections, 2 MB/s each except part 3 at 128 KB/s (rangeserve.py
#           slow=2:128): finished pieces take over the slow part; time, SHA-256, connection ends
#   resume  64 MiB at 1 MB/s per connection; Krunk closes after 5 s, starts again: the download
#           is paused where it stood; Resume goes on (no range again from 0), SHA-256 checked
#   change  12 MiB; paused, then the file "changes" on the server (rangeserve change=5): Resume
#           finds another version and the engine downloads it again, saying why
#   flight  the download glyph's flight and the page's bar recorded at the display's rate
param([string]$Phase = 'all')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "dl070"
$root = "$K\www\8093"
$dl = "$env:USERPROFILE\Downloads"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class SPI { [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, IntPtr c, uint d); }' -ErrorAction SilentlyContinue
[SPI]::SystemParametersInfo(0x1043, 0, [IntPtr]1, 3) | Out-Null    # animations on
New-Item -ItemType Directory -Force $root | Out-Null
if (-not (Test-Path "$root\big.bin")) { $b = New-Object byte[] (64MB); (New-Object Random 11).NextBytes($b); [IO.File]::WriteAllBytes("$root\big.bin", $b) }
if (-not (Test-Path "$root\mid.bin")) { $b = New-Object byte[] (12MB); (New-Object Random 13).NextBytes($b); [IO.File]::WriteAllBytes("$root\mid.bin", $b) }
if (-not (Test-Path "$root\small.bin")) { $b = New-Object byte[] (3MB); (New-Object Random 12).NextBytes($b); [IO.File]::WriteAllBytes("$root\small.bin", $b) }
Set-Content "$root\dl7.html" @'
<!doctype html><title>Downloads 0.7.0</title><style>body{font:22px sans-serif;padding:20px 40px}a{display:block;padding:14px;background:#def;margin:10px 0}</style>
<a id="steal" href="big.bin?kbps=2048&slow=2:128&of=4&v=steal">steal: 64 MiB, one slow connection</a>
<a id="resume" href="big.bin?kbps=1024&v=resume">resume: 64 MiB at 1 MB/s per connection</a>
<a id="change" href="mid.bin?kbps=1024&change=5&v=change">change: 12 MiB, changes after 5 requests</a>
<a id="small" href="small.bin">small file (one connection)</a>
<a id="fly" href="mid.bin?kbps=4096&v=fly">flight: 12 MiB</a>
'@
Copy-Item "$K\rangeserve.py" "$root\..\rangeserve.py" -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Remove-Item "$root\..\rangeserve.log" -ErrorAction SilentlyContinue
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$root\..\rangeserve.py`"", "`"$root`"", '8093'
Start-Sleep 2
foreach ($f in 'big*', 'mid*', 'small*') { Get-ChildItem $dl -Filter $f -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue }
$profile = "$K\p070dl"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
function Start-Krunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $script:A | Out-Null
  $script:c = ClientRect $script:A
}
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# The links on dl7.html (page top = 112 DIP below the client top, 64 DIP per link).
function LinkY([int]$i) { Y (112 + 20 + 10 + 28 + $i * 64) }
function Hash([string]$p) { if (Test-Path $p) { (Get-FileHash -Algorithm SHA256 $p).Hash.Substring(0, 16) } else { 'missing' } }
function WaitFile([string]$name, [int]$seconds, [long]$size) {
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalSeconds -lt $seconds) {
    $f = Get-Item "$dl\$name" -ErrorAction SilentlyContinue
    if ($f -and $f.Length -eq $size -and -not (Test-Path "$dl\$name.krunkpart") -and -not (Get-ChildItem $dl -Filter '*.crdownload' -ErrorAction SilentlyContinue)) {
      Start-Sleep -Milliseconds 500
      return ((Get-Date) - $t0).TotalSeconds
    }
    Start-Sleep -Milliseconds 250
  }
  return -1
}
function Stored {
  python -c "import sqlite3,json,sys; d=sqlite3.connect(sys.argv[1]); s=json.loads(d.execute('SELECT value FROM kv WHERE key=?',('downloads',)).fetchone()[0]); print(json.dumps([{k: i.get(k) for k in ('id','fileName','state','reason','received','connections')} | {'resume': (len(i['resume']['segments']) if i.get('resume') else None)} for i in s['items']]))" "$profile\Krunk\browser.db"
}
function KrunkLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern) -join ' ## ' }
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
function RowMenu([string]$item) {
  # The newest row of the downloads page: focus it (a click in the row's empty right part), open
  # its menu with Shift+F10; the first item is Pause for a running download, Resume for a
  # paused one.
  Click (X 900) (Y 205) 300
  SafeKeys '+{F10}' 600
  Shot "$P-menu-$item"
  SafeKeys '{ENTER}' 800
}

Start-Krunk
SafeGo 'http://127.0.0.1:8093/dl7.html' 2500
Shot "$P-0-page"
if ($Phase -in 'all', 'steal') {
  $t0 = Get-Date
  Click (X 300) (LinkY 0) 300
  Start-Sleep 3
  Click (X (($c.Right - $c.Left) / $c.Scale - 92)) (Y 56) 900
  Shot "$P-1-steal-popup-3s"
  Start-Sleep 3
  Shot "$P-2-steal-popup-6s"
  Keys '{ESC}' 300
  SafeKeys '^j' 1500
  Shot "$P-3-steal-page"
  $s = WaitFile 'big.bin' 150 64MB
  $total = ((Get-Date) - $t0).TotalSeconds
  Shot "$P-4-steal-done"
  Log "[$P] steal: big.bin after $([int]$total) s (wait $([int]$s)); sha256 $(Hash "$dl\big.bin") source $(Hash "$root\big.bin"); a static split would need 16 MiB at 128 KB/s = 128 s for the slow part"
  Log "[$P] steal log: $(KrunkLog 'download (\d+) (started|finished)|connection \d+ done')"
  Log "[$P] server: $((Get-Content "$root\..\rangeserve.log" | Select-String 'v=steal|slow') -join ' ## ')"
  SafeKeys '^w' 800
}
if ($Phase -in 'all', 'resume') {
  Focus $A | Out-Null
  Click (X 300) (LinkY 1) 300
  Start-Sleep 5
  Log "[$P] resume: stored before closing: $(Stored)"
  Quit
  Log "[$P] resume: after closing: $(Stored); file $((Get-Item "$dl\big (1).bin.krunkpart", "$dl\big.bin.krunkpart" -ErrorAction SilentlyContinue | ForEach-Object { $_.Name + ' ' + $_.Length }) -join ', ')"
  Start-Krunk
  SafeKeys '^j' 2000
  Shot "$P-5-resume-paused"
  RowMenu 'Resume'
  Start-Sleep 2
  Shot "$P-6-resume-running"
  $name = if (Test-Path "$dl\big (1).bin.krunkpart") { 'big (1).bin' } else { 'big.bin' }
  $s = WaitFile $name 90 64MB
  Shot "$P-7-resume-done"
  Log "[$P] resume: $name after $([int]$s) s more; sha256 $(Hash "$dl\$name") source $(Hash "$root\big.bin")"
  Log "[$P] resume log: $(KrunkLog 'resum|same file|download (\d+) finished')"
  Log "[$P] server after the restart: $((Get-Content "$root\..\rangeserve.log" | Select-String 'v=resume' | Select-Object -Last 8) -join ' ## ')"
}
if ($Phase -in 'all', 'change') {
  SafeGo 'http://127.0.0.1:8093/dl7.html' 2000
  Click (X 300) (LinkY 2) 1500
  SafeKeys '^j' 1200
  RowMenu 'Pause'
  Start-Sleep 1
  Shot "$P-8-change-paused"
  RowMenu 'Resume'
  Start-Sleep 4
  Shot "$P-9-change-restarted"
  $s = WaitFile 'mid.bin' 40 12MB
  Log "[$P] change: mid.bin after $([int]$s) s; sha256 $(Hash "$dl\mid.bin") source $(Hash "$root\mid.bin")"
  Log "[$P] change: stored $(Stored)"
  Log "[$P] change log: $(KrunkLog 'changed|again|one connection|download (\d+) (started|finished|failed)')"
  Shot "$P-10-change-page"
}
if ($Phase -in 'all', 'flight') {
  SafeGo 'http://127.0.0.1:8093/dl7.html' 2000
  $rect = "$($c.Left),$($c.Top),$($c.Right - $c.Left),$($c.Bottom - $c.Top)"
  $cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '2', '--seconds', '3', '--out', "$Shots\$P-flight" -WindowStyle Hidden -PassThru
  Start-Sleep -Milliseconds 700
  Click (X 300) (LinkY 4) 100
  $cap.WaitForExit(15000) | Out-Null
  Log "[$P] flight capture: $(Get-Content "$Shots\$P-flight.json")"
  SafeKeys '^j' 1500
  SafeGo 'http://127.0.0.1:8093/dl7.html' 1500
  Click (X 300) (LinkY 1) 300
  SafeKeys '^j' 1500
  $cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '1', '--seconds', '4', '--out', "$Shots\$P-bars" -WindowStyle Hidden -PassThru
  $cap.WaitForExit(20000) | Out-Null
  Shot "$P-11-bars"
  Log "[$P] bars capture: $(Get-Content "$Shots\$P-bars.json")"
}
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Copy-Item "$root\..\rangeserve.log" "$Shots\$P-rangeserve.log" -ErrorAction SilentlyContinue
Quit
Log "[$P] done"
