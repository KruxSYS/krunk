# Krunk 0.6.0 milestone 2 (R5): live resize. Edge and corner drags re-lay out the shell and the
# page while the mouse button is held (shots mid-drag), Esc during a drag restores the size,
# and the same with DevTools docked. -Dpi 100|125.
param([int]$Dpi = 100)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "resize-$Dpi"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
& powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent $Dpi | ForEach-Object { Log "[$P] dpi: $_" }
Start-Sleep 2
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "C:\pipeline\krunk\p060r-$Dpi"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 800, 600, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
SafeGo 'http://localhost:8094/resize.html' 2500
Shot "$P-0-before"
function Press([int]$x, [int]$y) { [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 300; [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 400 }
function Release { [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 700 }
# Right edge, 8 steps of 40 px, a shot every second step while the button is down.
$r = WindowRect $A; $x1 = $r.Right - 1; $y1 = [int](($r.Top + $r.Bottom) / 2)
Press $x1 $y1
for ($i = 1; $i -le 8; $i++) { AbsMove ($x1 + 40 * $i) $y1; Start-Sleep -Milliseconds 120; if ($i % 2 -eq 0) { Shot "$P-1-right-$i"; $w = WindowRect $A; Log "[$P] right step $i window right=$($w.Right)" } }
Start-Sleep -Milliseconds 800; Shot "$P-1-right-held"
Release; Shot "$P-1-right-released"
# Bottom-right corner inwards.
$r = WindowRect $A; $x1 = $r.Right - 2; $y1 = $r.Bottom - 2
Press $x1 $y1
for ($i = 1; $i -le 6; $i++) { AbsMove ($x1 - 40 * $i) ($y1 - 30 * $i); Start-Sleep -Milliseconds 120; if ($i % 2 -eq 0) { Shot "$P-2-corner-$i" } }
Release; Shot "$P-2-corner-released"
# Left edge, then Esc restores the start size while the button is still down.
$r = WindowRect $A; $x1 = $r.Left + 1; $y1 = [int](($r.Top + $r.Bottom) / 2)
Log "[$P] before left drag: $($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)"
Press $x1 $y1
for ($i = 1; $i -le 4; $i++) { AbsMove ($x1 + 40 * $i) $y1; Start-Sleep -Milliseconds 120 }
Shot "$P-3-left-dragged"; $w = WindowRect $A; Log "[$P] left dragged: $($w.Left),$($w.Top)-$($w.Right),$($w.Bottom)"
Keys '{ESC}' 400; Shot "$P-3-left-esc"; $w = WindowRect $A; Log "[$P] after Esc: $($w.Left),$($w.Top)-$($w.Right),$($w.Bottom)"
Release; Shot "$P-3-left-released"
# Top edge with DevTools docked (right): both panes follow.
SafeKeys '{F12}' 3000
Shot "$P-4-devtools"
$r = WindowRect $A; $x1 = [int](($r.Left + $r.Right) / 2); $y1 = $r.Top + 1
Press $x1 $y1
for ($i = 1; $i -le 5; $i++) { AbsMove $x1 ($y1 + 30 * $i); Start-Sleep -Milliseconds 120; if ($i % 2 -eq 1) { Shot "$P-4-top-$i" } }
Release; Shot "$P-4-top-released"
SafeKeys '{F12}' 1500
# Maximize/restore still work (double-click on the caption).
DoubleClick ([int](($r.Left + $r.Right) / 2)) ($r.Top + 12); Start-Sleep 1; Shot "$P-5-maximized"
DoubleClick 640 12; Start-Sleep 1; Shot "$P-5-restored"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
if ($Dpi -ne 100) { & powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent 100 | ForEach-Object { Log "[$P] dpi: $_" } }
Log "[$P] done"
