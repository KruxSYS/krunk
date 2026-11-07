# Krunk 0.6.0 milestone 5 (R12, R13): the download-start animation (a frame burst right after
# a click on a download link: the flying glyph, then the pulsing button) and the downloads
# popup/page with speed, time left and one bar segment per connection (rangeserve.py, 4
# connections at 1 MB/s each). Also the reduce-motion run (-ReduceMotion: Windows "Animation
# effects" off → badge/pulse only).
param([switch]$ReduceMotion)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = if ($ReduceMotion) { 'dl-rm' } else { 'dl' }
$root = 'C:\pipeline\krunk\www\8093'
$dl = "$env:USERPROFILE\Downloads"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
# Windows "Animation effects" = SPI_SETCLIENTAREAANIMATION (the system reduced-motion switch).
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class SPI { [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, IntPtr c, uint d); [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, ref bool c, uint d); }' -ErrorAction SilentlyContinue
$animBefore = $false; [SPI]::SystemParametersInfo(0x1042, 0, [ref]$animBefore, 0) | Out-Null
[SPI]::SystemParametersInfo(0x1043, 0, [IntPtr]$(if ($ReduceMotion) { 0 } else { 1 }), 3) | Out-Null
Log "[$P] client area animation before: $animBefore; now: $(-not $ReduceMotion)"
New-Item -ItemType Directory -Force $root | Out-Null
if (-not (Test-Path "$root\big.bin")) {
  $b = New-Object byte[] (64MB); (New-Object Random 11).NextBytes($b); [IO.File]::WriteAllBytes("$root\big.bin", $b)
}
Set-Content "$root\dl.html" '<!doctype html><title>Download test</title><style>body{font:28px sans-serif;padding:40px}a{display:block;padding:30px;background:#def;margin:20px 0}</style><a id="dl" href="big.bin?kbps=1024&via=link">download big.bin (4 connections, 1 MB/s each)</a><a id="small" href="small.bin">small file (one connection)</a>'
if (-not (Test-Path "$root\small.bin")) { $b = New-Object byte[] (3MB); (New-Object Random 12).NextBytes($b); [IO.File]::WriteAllBytes("$root\small.bin", $b) }
Copy-Item C:\pipeline\krunk\rangeserve.py "$root\..\rangeserve.py" -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$root\..\rangeserve.py`"", "`"$root`"", '8093'
Start-Sleep 2
Get-ChildItem $dl -Filter 'big*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
Get-ChildItem $dl -Filter 'small*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
$profile = "C:\pipeline\krunk\p060d-$P"
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
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
SafeGo 'http://127.0.0.1:8093/dl.html' 3000
Shot "$P-0-page"
# (R12) Click the link, then a burst of shots every ~60 ms for 1.2 s.
$x = X 300; $y = Y 225
[U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 150
[SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 40; [SI]::Mouse(4, 0, 0) | Out-Null
$t0 = Get-Date
for ($i = 0; $i -lt 18; $i++) { Shot "$P-1-flight-$i"; Start-Sleep -Milliseconds 40 }
Log "[$P] burst of 18 shots over $([int]((Get-Date) - $t0).TotalMilliseconds) ms"
Start-Sleep 2
# (R13) The button's popup: speed, time left, one segment per connection.
Click (X (($c.Right - $c.Left) / $c.Scale - 92)) (Y 56) 1500
Shot "$P-2-popup-segments"
Start-Sleep 2
Shot "$P-3-popup-segments-later"
Keys '{ESC}' 500
SafeKeys '^j' 2500
Shot "$P-4-page-segments"
Start-Sleep 3
Shot "$P-5-page-segments-later"
# A single-connection download shows one bar.
SafeGo 'http://127.0.0.1:8093/dl.html' 2500
Click (X 300) (Y 335) 600
Start-Sleep 1
SafeKeys '^j' 2500
Shot "$P-6-page-single-bar"
Log "[$P] downloads log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'download') -join ' ## ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
[SPI]::SystemParametersInfo(0x1043, 0, [IntPtr]$(if ($animBefore) { 1 } else { 0 }), 3) | Out-Null
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Log "[$P] done"
