# Krunk 0.3.0 acceptance 5: DevTools docked in its tab's window. Fresh profile; needs
# serve.ps1 on localhost:8092. Phase "main" runs a-g; phase "restart" checks that the dock
# side and size persisted; phase "fullscreen" that F11 keeps it and page full screen hides it.
# Screenshots "dt-*".
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "dt-$Phase"
$profile = 'C:\pipeline\krunk\p030d'
$env:LOCALAPPDATA = $profile
function Start-Krunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
}
function DragFrom([int]$x1, [int]$y1, [int]$x2, [int]$y2) { Drag $x1 $y1 $x2 $y2 15; Start-Sleep 1 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}
if ($Phase -eq 'main') {
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
}
Start-Krunk
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] no focus"; throw 'no focus' }
$c = ClientRect $A
$page = [int]($c.Top + 104 * $c.Scale)

if ($Phase -eq 'main') {
  SafeGo 'http://localhost:8092/move.html?dt=1' 3500
  SafeKeys '^t' 1500; SafeGo 'https://example.com/' 3000
  SafeKeys '^1' 1500

  # (a) F12: docked inside the window, no top-level DevTools window.
  SafeKeys '{F12}' 4000
  Shot "$P-a-f12-docked"
  KrunkWindows "$P-a"
  # (g) F12 again (typed inside DevTools) closes it.
  Focus $A | Out-Null
  Keys '{F12}' 2500
  Shot "$P-g-f12-closed"
  KrunkWindows "$P-g"

  # (b) Right-click -> Inspect on the page heading selects it.
  RightClick ([int]($c.Left + 80)) ($page + 45) 1200
  Shot "$P-b1-context-menu"
  Keys '{UP}{ENTER}' 4000
  Shot "$P-b2-inspect-heading"

  # Keys typed in DevTools: Ctrl+F stays with DevTools, Ctrl+T reaches Krunk.
  Click ([int]($c.Right - 300)) ($page + 300) 500
  Keys '^f' 1200
  Shot "$P-k1-ctrl-f-in-devtools"
  Keys '{ESC}' 500
  Keys '^t' 2000
  Shot "$P-k2-ctrl-t-from-devtools"
  Keys '^w' 1500
  # Closing that tab activated its neighbour: back to tab 1 (with DevTools).
  SafeKeys '^1' 1500

  # (d) Drag the splitter 200 px left, then dock at the bottom and drag that splitter up.
  $sx = [int]($c.Right - (480 + 2.5) * $c.Scale)
  DragFrom $sx ($page + 250) ($sx - 200) ($page + 250)
  Shot "$P-d1-splitter-dragged"
  # The dock control sits left of the shield button.
  Click ([int]($c.Right - 118)) ([int]($c.Top + 56)) 2000
  Shot "$P-d2-docked-bottom"
  $sy = [int]($c.Bottom - (320 + 2.5) * $c.Scale)
  DragFrom ([int]($c.Left + 400)) $sy ([int]($c.Left + 400)) ($sy - 100)
  Shot "$P-d3-bottom-splitter-dragged"

  # (c) Tab 2 has no DevTools; give it its own, then back to tab 1: each keeps its own.
  Focus $A | Out-Null
  SafeKeys '^2' 1500
  Shot "$P-c1-tab2-no-devtools"
  SafeKeys '{F12}' 3500
  Shot "$P-c2-tab2-own-devtools"
  Focus $A | Out-Null
  Keys '^1' 2000
  Shot "$P-c3-tab1-devtools-again"

  # (f) Move tab 1 (with DevTools) to a new window: still docked there.
  $t1 = TabPoint $A 0 2
  RightClick $t1[0] $t1[1] 1200
  Click ($t1[0] + 60) ($t1[1] + 81) 3500
  $B = [U]::GetForegroundWindow()
  [U]::MoveWindow($B, 200, 60, 1000, 680, $true) | Out-Null; Start-Sleep 1.5
  Shot "$P-f-moved-with-devtools"
  KrunkWindows "$P-f"

  # (e) Close that tab: its DevTools closes with it (and the window, its last tab).
  Focus $B | Out-Null
  Keys '^w' 2500
  Shot "$P-e-tab-closed"
  KrunkWindows "$P-e"
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
} elseif ($Phase -eq 'fullscreen') {
  # F11 keeps DevTools (as in Chrome); page full screen hides it until Esc.
  SafeGo 'http://localhost:8092/move.html?fs=1' 3500
  SafeKeys '{F12}' 4000
  Shot "$P-0-docked"
  Click 200 ($page + 150) 500
  SafeKeys '{F11}' 2500
  Shot "$P-1-f11-keeps-devtools"
  SafeKeys '{F11}' 2500
  # The video (page y 163..433) is still under page + 200 with DevTools docked at the bottom.
  DoubleClick 250 ($page + 200); Start-Sleep 3
  Shot "$P-2-page-fullscreen-hides"
  Keys '{ESC}' 2500
  Shot "$P-3-back-docked"
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
} else {
  # The dock side and size persisted: F12 on the restored tab.
  SafeKeys '{F12}' 3500
  Shot "$P-restored-docked"
  KrunkWindows $P
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
}
Log "[$P] processes after closing: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Copy-Item "$profile\Krunk\browser.db" "C:\pipeline\krunk\p030d-$Phase.db" -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'DevTools|inspect|tab |window|full screen|WARN|ERROR' | ForEach-Object { Log "[$P] log: $_" }
