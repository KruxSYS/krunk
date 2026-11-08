$Build = 'dev'
# Krunk 0.7.1 M4 (R10): the download glyph's flight recorded 3 times with tools\framecap.exe
# (scripts/frames/analyze.py track --ease 450 measures each frame's distance from the eased
# curve). $Build 'dev' = C:\pipeline\krunk\app, 'installed' = the installed release
# (%LOCALAPPDATA%\Programs\Krunk, before the test profile replaces LOCALAPPDATA).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "flight-$Build"
$App = if ($Build -eq 'installed') { Join-Path $env:LOCALAPPDATA 'Programs\Krunk' } else { "$K\app" }
$root = "$K\www\8093"
$dl = "$env:USERPROFILE\Downloads"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class SPI { [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, IntPtr c, uint d); }' -ErrorAction SilentlyContinue
[SPI]::SystemParametersInfo(0x1043, 0, [IntPtr]1, 3) | Out-Null    # animations on
New-Item -ItemType Directory -Force $root | Out-Null
if (-not (Test-Path "$root\mid.bin")) { $b = New-Object byte[] (12MB); (New-Object Random 13).NextBytes($b); [IO.File]::WriteAllBytes("$root\mid.bin", $b) }
Set-Content "$root\fly.html" @'
<!doctype html><title>Flight</title><style>body{font:22px sans-serif;padding:20px 40px}a{display:block;padding:14px;background:#def;margin:10px 0}</style>
<a id="fly" href="mid.bin?kbps=4096&v=fly">flight: 12 MiB</a>
'@
Copy-Item "$K\rangeserve.py" "$root\..\rangeserve.py" -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$root\..\rangeserve.py`"", "`"$root`"", '8093'
Start-Sleep 2
$profile = "$K\p071fl"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Log "[$P] Krunk.dll $((Get-FileHash "$App\Krunk.dll").Hash.Substring(0, 16)) from $App"
Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$rect = "$($c.Left),$($c.Top),$($c.Right - $c.Left),$($c.Bottom - $c.Top)"
for ($n = 1; $n -le 3; $n++) {
  Get-ChildItem $dl -Filter 'mid*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
  SafeGo 'http://127.0.0.1:8093/fly.html' 2500
  $cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '2', '--seconds', '3', '--out', "$Shots\$P-$n" -WindowStyle Hidden -PassThru
  Start-Sleep -Milliseconds 700
  Click (X 300) (Y (112 + 20 + 10 + 28)) 100
  $cap.WaitForExit(15000) | Out-Null
  Log "[$P] capture ${n}: $(Get-Content "$Shots\$P-$n.json")"
  Start-Sleep 4
}
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-ChildItem $dl -Filter 'mid*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
Log "[$P] done"
