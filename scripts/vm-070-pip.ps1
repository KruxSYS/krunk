# Krunk 0.7.0 M5 (R8): the volume bar of the floating video window. On www/8094/pipvol.html (one
# video with sound; the window title shows "vol NN on|muted") Alt+P puts the video in the
# engine's floating window; Krunk's bar appears under it, hides when the pointer leaves, comes
# back on hover; the slider, the wheel over the floating window and the bar's mute button
# change the page's volume; the page's own changes move the bar; the bar follows the window
# when it is dragged; leaving PiP hides it.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'pipvol'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p070pv'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1000, 700, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PipWindow {
  $pids = KrunkPids
  [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -notlike 'Krunk*' -and [TW]::Txt($_) -ne '' } | Select-Object -First 1
}
function Bar {
  $pids = KrunkPids
  $h = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($h) { WindowRect $h } else { $null }
}
function RectText($r) { if ($r) { "$($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { 'none' } }
function State([string]$what) {
  $h = PipWindow
  $pr = if ($h) { WindowRect $h } else { $null }
  Log "[$P] ${what}: page '$([TW]::Txt($A))' | floating $(RectText $pr) | bar $(RectText (Bar))"
  $pr
}
function Away { MoveTo (X 500) (Y 560) }
function PageKey([string]$k) { Focus $A | Out-Null; Click (X 700) (Y 160) 300; Keys $k 800 }

SafeGo 'http://localhost:8094/pipvol.html' 4000
PageKey 'm'                                                                    # sound on (the page's own control)
State 'loaded' | Out-Null
SafeKeys '%p' 1000
$pr = State 'Alt+P: in the floating window, bar shown at first'
Shot "$P-1-entered"
Away; Start-Sleep 3
State 'pointer away 3 s: bar hidden' | Out-Null
Shot "$P-2-away"
if (-not $pr) { Log "[$P] no floating window"; throw 'no pip' }
$cx = [int](($pr.Left + $pr.Right) / 2); $cy = [int](($pr.Top + $pr.Bottom) / 2)
MoveTo $cx $cy; Start-Sleep -Milliseconds 700
$b = Bar
State 'hover over the floating window: bar back' | Out-Null
Shot "$P-3-hover"
if ($b) {
  $s = $c.Scale
  $l = $b.Left + 46 * $s; $r = $b.Right - 74 * $s; $y = [int](($b.Top + $b.Bottom) / 2)
  MoveTo ([int]$l) $y; Start-Sleep -Milliseconds 200
  Click ([int]($l + 0.3 * ($r - $l))) $y 900
  State 'slider clicked at 30 %' | Out-Null
  Shot "$P-4-slider-30"
}
Wheel $cx $cy -3
State 'wheel up 3 notches over the floating window' | Out-Null
Shot "$P-5-wheel-up"
Wheel $cx $cy 2
State 'wheel down 2 notches' | Out-Null
$b = Bar
if ($b) {
  Click ([int]($b.Left + 21 * $c.Scale)) ([int](($b.Top + $b.Bottom) / 2)) 900
  State 'bar mute button' | Out-Null
  Shot "$P-6-muted"
  Click ([int]($b.Left + 21 * $c.Scale)) ([int](($b.Top + $b.Bottom) / 2)) 900
  State 'bar mute button again' | Out-Null
}
# The floating window's own mute button (bottom right, left of the captions button).
MoveTo $cx $cy; Start-Sleep -Milliseconds 500
Click ($pr.Right - 58) ($pr.Bottom - 18) 900
MoveTo $cx $cy; Start-Sleep -Milliseconds 500
State 'the floating window''s own mute button' | Out-Null
Shot "$P-6b-engine-mute"
Click ($pr.Right - 58) ($pr.Bottom - 18) 900
State 'the floating window''s mute button again' | Out-Null
PageKey '8'
MoveTo $cx $cy; Start-Sleep -Milliseconds 700
State 'page set 80 %' | Out-Null
Shot "$P-7-page-80"
PageKey 'm'
MoveTo $cx $cy; Start-Sleep -Milliseconds 700
State 'page muted' | Out-Null
Shot "$P-8-page-muted"
PageKey 'm'
# Drag the floating window up and left: the bar goes with it.
$gx = $pr.Left + 60; $gy = [int]($pr.Top + ($pr.Bottom - $pr.Top) * 0.3)
Drag $gx $gy ($gx - 400) ($gy - 250) 25
Start-Sleep -Milliseconds 600
$pr2 = State 'floating window dragged'
Shot "$P-9-moved"
Focus $A | Out-Null
SafeKeys '%p' 2000
Away; Start-Sleep 1
State 'Alt+P: left the floating window' | Out-Null
Shot "$P-10-left"
# The toolbar button and Alt+P again (the other triggers are in vm-060-pip).
SafeKeys '%p' 2500
State 'in again' | Out-Null
Focus $A | Out-Null
SafeKeys '%p' 2000
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'picture|floating|volume') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
