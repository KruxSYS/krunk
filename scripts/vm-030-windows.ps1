# Krunk 0.3.0 acceptance 4 (multi-window) on the VM, in a fresh profile (LOCALAPPDATA points
# to C:\pipeline\krunk\p030). Needs serve.ps1 on localhost:8092 (move.html) — started here.
# Screenshots "win-*", lines in run.log.
param([switch]$KeepProfile)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'win'
function DragVia([int]$x1, [int]$y1, [int]$xm, [int]$ym, [int]$x2, [int]$y2) {
  [U]::SetCursorPos($x1, $y1) | Out-Null; AbsMove $x1 $y1; Start-Sleep -Milliseconds 150
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 300
  foreach ($seg in @(@($x1, $y1, $xm, $ym), @($xm, $ym, $x2, $y2))) {
    for ($i = 1; $i -le 15; $i++) { AbsMove ($seg[0] + ($seg[2] - $seg[0]) * $i / 15) ($seg[1] + ($seg[3] - $seg[1]) * $i / 15); Start-Sleep -Milliseconds 30 }
  }
  Start-Sleep -Milliseconds 400; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 1500
}
function AccessCount([string]$like) { @(Get-Content C:\pipeline\krunk\www\access.log | Where-Object { $_ -like $like }).Count }

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}
$profile = 'C:\pipeline\krunk\p030'
if (-not $KeepProfile) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 640, 700, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[a] could not focus A'; throw 'no focus' }

# Window A: [move test (history 3, typed field, scrolled, video playing), example.com, about]
Go 'http://localhost:8092/move.html?step=1' 3000
Go 'http://localhost:8092/move.html?step=2' 4000
$c = ClientRect $A
$page = [int]($c.Top + 104 * $c.Scale)
Click ([int]($c.Left + 250)) ($page + 98) 300
Keys 'typed before move' 500
Click ([int]($c.Left + 560)) ($page + 500) 300
Keys '{DOWN}{DOWN}' 800
Keys '^t' 1500; Go 'https://example.com/' 3000
Keys '^t' 1500; Go 'krunk://about' 2000
$t0 = TabPoint $A 0
Click $t0[0] $t0[1] 1500
$gets = AccessCount '*8092/move.html?step=2*'
Shot "$P-a1-before"
Log "[a] before: GETs of move.html?step=2 = $gets"

# (a) Drag the move test tab out of the strip: a new window, no reload.
DragVia $t0[0] $t0[1] ($t0[0] + 20) ($t0[1] + 120) ($t0[0] + 700) ($t0[1] + 120)
Start-Sleep 3
$B = FrameByTitle 'Move test*'
Shot "$P-a2-dragged-out"
KrunkWindows 'a'
Log "[a] after: GETs of move.html?step=2 = $(AccessCount '*8092/move.html?step=2*') (no reload if unchanged)"
Start-Sleep 3
Shot "$P-a3-dragged-out-3s-later"
[U]::MoveWindow($B, 640, 0, 640, 700, $true) | Out-Null; Start-Sleep 1

# (b) Drag example.com from A onto B's strip, right of B's tab: it lands at index 1.
$A = FrameByTitle 'Example Domain*'
$ta = TabPoint $A 0
$tb = TabPoint $B 0 1
DragVia $ta[0] $ta[1] ($ta[0]) ($ta[1] + 150) ($tb[0] + 180) ($tb[1])
Start-Sleep 2
Shot "$P-b-dropped-on-b"
KrunkWindows 'b'

# (c) Tab menu "Move to new window" on B's second tab, then Ctrl+N.
$B = FrameByTitle 'Example Domain*'
$t1 = TabPoint $B 1 2
RightClick $t1[0] $t1[1] 1200
Shot "$P-c1-tab-menu"
Click ($t1[0] + 60) ($t1[1] + 81) 2500
$C = [U]::GetForegroundWindow()
Shot "$P-c2-moved-to-new-window"
KrunkWindows 'c-move'
[U]::MoveWindow($C, 0, 360, 640, 400, $true) | Out-Null; Start-Sleep 1
Keys '^n' 3000
$D = [U]::GetForegroundWindow()
[U]::MoveWindow($D, 640, 400, 640, 380, $true) | Out-Null; Start-Sleep 1
Shot "$P-c3-ctrl-n"
KrunkWindows 'c-ctrl-n'

# (d) Drag D's only tab onto C's strip: they merge and D closes.
$td = TabPoint $D 0
$tc = TabPoint $C 0
DragVia $td[0] $td[1] ($td[0]) ($td[1] + 150) ($tc[0] + 180) ($tc[1])
Start-Sleep 2
Shot "$P-d-merged"
KrunkWindows 'd'

# (h) The popup menu, shield, find and a shortcut in a second window (C).
[U]::SetForegroundWindow($C) | Out-Null; Start-Sleep -Milliseconds 500
$tc0 = TabPoint $C 0
Click $tc0[0] $tc0[1] 1200
$cc = ClientRect $C
Click ([int]($cc.Right - 22)) ([int]($cc.Top + 56)) 1200
Shot "$P-h1-menu-in-c"
Keys '{ESC}' 600
Click ([int]($cc.Right - 56)) ([int]($cc.Top + 56)) 1200
Shot "$P-h2-shield-in-c"
Keys '{ESC}' 600
Click ([int]($cc.Left + 300)) ([int]($cc.Top + 250)) 300
Keys '^f' 600; Keys 'domain' 800
Shot "$P-h3-find-in-c"
Keys '{ESC}' 500
Keys '^t' 1500
Shot "$P-h4-ctrl-t-in-c"
KrunkWindows 'h'

# (g) A private tab moved to a new window stays private and stays out of history.
Keys '^n' 3000
$E = [U]::GetForegroundWindow()
[U]::MoveWindow($E, 100, 100, 1000, 600, $true) | Out-Null; Start-Sleep 1
Keys '^+n' 1500
Go 'http://localhost:8092/move.html?private=1' 3000
$tp = TabPoint $E 1 2
RightClick $tp[0] $tp[1] 1200
Click ($tp[0] + 60) ($tp[1] + 81) 2500
$F = [U]::GetForegroundWindow()
Shot "$P-g1-private-moved"
KrunkWindows 'g'
Keys '^l' 300; Keys 'http://localhost:8092/move.html?private=2' 100; Keys '{ENTER}' 3000
Shot "$P-g2-private-navigated"
[U]::SetForegroundWindow($E) | Out-Null; Start-Sleep -Milliseconds 500
Go 'krunk://history' 2500
Shot "$P-g3-history"

# (e) Close every window: Krunk exits, no process left.
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 6
Log "[e] Krunk processes after closing all windows: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Copy-Item "$profile\Krunk\browser.db" C:\pipeline\krunk\p030-browser.db -Force
Get-Content "$profile\Krunk\krunk.log" | ForEach-Object { Log "[log] $_" }
