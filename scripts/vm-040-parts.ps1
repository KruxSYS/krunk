# Krunk 0.4.0 milestone 6: large downloads fetched in parts. Serves www\8093 with
# vm-www/rangeserve.py (Range, ETag, throttling per connection, a file that changes, a cookie
# check) and downloads a 64 MB file in several ways; every request the server got is in
# rangeserve.log, the outcome (time, SHA-256, files left) in run.log. Fresh profile.
#   A  one connection (the server refuses ranges), 2 MB/s per connection
#   B  four connections, 2 MB/s each
#   C  the file changes on the server during the download: it fails, no parts are left
#   D  no validator: one connection
#   E  cookie and referrer reach the server on every part
#   F  pause and resume (row menu), then the file is complete
#   G  cancel (row menu): no parts are left
#   H  quit while it runs: the next start removes the part files
param([string]$Only = '')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'parts'
$root = 'C:\pipeline\krunk\www\8093'
$dl = "$env:USERPROFILE\Downloads"
$base = 'http://127.0.0.1:8093'
# Row menu of a running download (CSS px below the click): Pause/Resume 21, Cancel 51.
$Menu = @{ PauseResume = 21; Cancel = 51 }

New-Item -ItemType Directory -Force $root | Out-Null
if (-not (Test-Path "$root\big.bin")) {
  $b = New-Object byte[] (64MB); (New-Object Random 11).NextBytes($b); [IO.File]::WriteAllBytes("$root\big.bin", $b)
}
Set-Content "$root\setcookie.html" '<!doctype html><title>Cookie</title><script>document.cookie = "kr=parts-test; path=/";</script><a id="dl" href="big.bin?cookie=kr&kbps=8192&via=link" style="display:block;font:40px sans-serif;padding:30px;background:#def">download big.bin (link)</a><p>cookie set</p>'
$want = (Get-FileHash "$root\big.bin").Hash
Copy-Item C:\pipeline\krunk\rangeserve.py "$root\..\rangeserve.py" -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Remove-Item "$root\..\rangeserve.log" -ErrorAction SilentlyContinue
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$root\..\rangeserve.py`"", "`"$root`"", '8093'
Start-Sleep 2
function Clean { Get-ChildItem $dl -Filter 'big*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue }
function Leftovers { @(Get-ChildItem $dl -Filter 'big*.part*' -ErrorAction SilentlyContinue).Count }
# Waits for big.bin to be complete (or $max seconds); logs the time and the hash.
function WaitFile([string]$label, [int]$max, [datetime]$t0) {
  for ($i = 0; $i -lt $max * 4; $i++) {
    $f = Get-ChildItem $dl -Filter 'big*.bin' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike '*.part*' -and $_.Length -eq 64MB } | Select-Object -First 1
    if ($f) {
      Start-Sleep -Milliseconds 300
      $s = [math]::Round(((Get-Date) - $t0).TotalSeconds, 1)
      $ok = (Get-FileHash $f.FullName).Hash -eq $want
      Log "[$label] complete after $s s, SHA-256 matches: $ok, part files left: $(Leftovers)"
      return $s
    }
    Start-Sleep -Milliseconds 250
  }
  Log "[$label] not complete after $max s; files: $((Get-ChildItem $dl -Filter 'big*' | ForEach-Object { "$($_.Name)=$($_.Length)" }) -join ', ')"
  return $null
}
function Go([string]$url) { SafeKeys '^1' 600; SafeGo $url 300 }

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Clean
$profile = 'C:\pipeline\krunk\p040p2'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
$page = [int]($c.Top + 104 * $c.Scale)
SafeGo "$base/setcookie.html" 1500
# The downloads page in tab 2 (for the row menu).
SafeKeys '^j' 1500
$rx = [int]($c.Left + 300 * $c.Scale); $ry = [int]($page + 95 * $c.Scale)

if (-not $Only -or 'A' -like $Only) {
  $t0 = Get-Date; Go "$base/big.bin?norange=1&kbps=2048"
  $a = WaitFile 'A one connection' 60 $t0; Clean
}
if (-not $Only -or 'B' -like $Only) {
  $t0 = Get-Date; Go "$base/big.bin?kbps=2048"
  Start-Sleep 3; SafeKeys '^2' 800; Shot "$P-B-running"
  $b = WaitFile 'B four connections' 60 $t0; Clean
  if ($a -and $b) { Log "[speed] one connection $a s, four $b s: $([math]::Round($a / $b, 2))x faster" }
}
if (-not $Only -or 'C' -like $Only) {
  Go "$base/big.bin?change=3&kbps=4096"
  Start-Sleep 6
  SafeKeys '^2' 800; Shot "$P-C-changed"
  Log "[C file changed] files: $((Get-ChildItem $dl -Filter 'big*' | ForEach-Object { $_.Name }) -join ', '); part files left: $(Leftovers)"
  Clean
}
if (-not $Only -or 'D' -like $Only) {
  $t0 = Get-Date; Go "$base/big.bin?noetag=1&kbps=8192"
  WaitFile 'D no validator' 60 $t0 | Out-Null; Clean
}
if (-not $Only -or 'E' -like $Only) {
  # Typed into the address bar: the engine sends no Referer, nor do the parts.
  $t0 = Get-Date; Go "$base/big.bin?cookie=kr&kbps=8192&via=typed"
  WaitFile 'E cookie, typed' 60 $t0 | Out-Null; Clean
  # A link on the page: the engine sends a Referer, and the parts the same one.
  SafeKeys '^1' 600; SafeGo "$base/setcookie.html" 1500
  $t0 = Get-Date; Click ([int]($c.Left + 200 * $c.Scale)) ([int]($page + 50 * $c.Scale)) 300
  WaitFile 'E cookie, link' 60 $t0 | Out-Null; Clean
}
if (-not $Only -or 'F' -like $Only) {
  $t0 = Get-Date; Go "$base/big.bin?kbps=1024"
  Start-Sleep 4
  SafeKeys '^2' 1000
  RightClick $rx $ry 800; Click ($rx + 60) ($ry + $Menu.PauseResume) 1500
  Shot "$P-F-paused"
  $sizes1 = (Get-ChildItem $dl -Filter 'big*.part*' | Measure-Object Length -Sum).Sum
  Start-Sleep 3
  $sizes2 = (Get-ChildItem $dl -Filter 'big*.part*' | Measure-Object Length -Sum).Sum
  Log "[F paused] part bytes $sizes1 then $sizes2 (no change while paused: $($sizes1 -eq $sizes2))"
  RightClick $rx $ry 800; Click ($rx + 60) ($ry + $Menu.PauseResume) 1000
  WaitFile 'F paused and resumed' 90 $t0 | Out-Null
  Shot "$P-F-done"; Clean
}
if (-not $Only -or 'G' -like $Only) {
  Go "$base/big.bin?kbps=512"
  Start-Sleep 4
  SafeKeys '^2' 1000
  RightClick $rx $ry 800; Click ($rx + 60) ($ry + $Menu.Cancel) 2500
  Shot "$P-G-cancelled"
  Log "[G cancelled] files: $((Get-ChildItem $dl -Filter 'big*' | ForEach-Object { $_.Name }) -join ', '); part files left: $(Leftovers)"
  Clean
}
if (-not $Only -or 'H' -like $Only) {
  # Quit while it runs: the part files stay until the next start, which removes them.
  Go "$base/big.bin?kbps=512"
  Start-Sleep 4
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  for ($i = 0; $i -lt 40 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
  Log "[H quit while running] part files after quitting: $(Leftovers)"
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  Log "[H next start] part files: $(Leftovers)"
  $A = MainWindow; Focus $A | Out-Null
  SafeKeys '^j' 1500
  Shot "$P-H-after-restart"
  Clean
}
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Clean
Get-Content "$profile\Krunk\krunk.log" | Select-String 'download|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
Get-Content "$root\..\rangeserve.log" | ForEach-Object { Log "[server] $_" }
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
