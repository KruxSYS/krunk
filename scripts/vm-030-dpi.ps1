# Krunk 0.3.0 acceptance 4(i): two windows at 125 % (the largest scale this 1280x800 display allows) (the VM has one monitor), a tab dragged
# from one strip onto the other, layouts checked by screenshot. Restores 100 % at the end.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'dpi'
function DragVia([int]$x1, [int]$y1, [int]$xm, [int]$ym, [int]$x2, [int]$y2) {
  [U]::SetCursorPos($x1, $y1) | Out-Null; AbsMove $x1 $y1; Start-Sleep -Milliseconds 150
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 300
  foreach ($seg in @(@($x1, $y1, $xm, $ym), @($xm, $ym, $x2, $y2))) {
    for ($i = 1; $i -le 15; $i++) { AbsMove ($seg[0] + ($seg[2] - $seg[0]) * $i / 15) ($seg[1] + ($seg[3] - $seg[1]) * $i / 15); Start-Sleep -Milliseconds 30 }
  }
  Start-Sleep -Milliseconds 400; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 1500
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
& powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent 125 | ForEach-Object { Log "[$P] dpi: $_" }
Start-Sleep 3
$profile = 'C:\pipeline\krunk\p030dpi'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
$c = ClientRect $A
Log "[$P] window A scale $($c.Scale)"
if (-not (Focus $A)) { Log "[$P] could not focus A"; throw "no focus" }
SafeGo 'https://example.com/' 3000
SafeKeys '^t' 1500; SafeGo 'krunk://about' 2500
SafeKeys '^n' 3000
$B = [U]::GetForegroundWindow()
[U]::MoveWindow($B, 640, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
Shot "$P-1-two-windows"
$ta = TabPoint $A 1 2
$tb = TabPoint $B 0
Log "[$P] A tab1 $($ta -join ','), B tab0 $($tb -join ',')"
DragVia $ta[0] $ta[1] $ta[0] ($ta[1] + 200) ($tb[0] + [int](150 * $c.Scale)) $tb[1]
Start-Sleep 2
Shot "$P-2-dropped"
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 4
Get-Content "$profile\Krunk\krunk.log" | Select-String 'tab |window|WARN|ERROR' | ForEach-Object { Log "[$P] log: $_" }
& powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent 100 | ForEach-Object { Log "[$P] dpi: $_" }
