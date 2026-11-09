# Krunk 0.11.0 VM flow A: bookmarks bar (items 1-3), favicon probe (4), tab overflow (8),
# source viewer (15) and search for selected text (16), on one fresh profile.
#   -Phase main     fresh profile, seeded bookmarks (vm-0110-seed.py), every check
#   -Phase restart  the same profile again: no second favicon probe (krunk.log)
# Needs serve.ps1 on 8094, 8101, 8102, 8103. Screenshots "a-<phase>-*", the profile's
# krunk.log lines in run.log.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "a-$Phase"
$profile = "$K\p0110a"
foreach ($port in 8094, 8101, 8102, 8103) {
  if (-not (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue)) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList',"http://*:$port/"
  }
}
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
}
function StopKrunk {
  foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1200 } }
  Start-Sleep 2
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
}
function MiddleClick([int]$x, [int]$y, [int]$wait = 500) {
  MoveTo $x $y; Start-Sleep -Milliseconds 80
  [U]::mouse_event(0x20, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [U]::mouse_event(0x40, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function ProbeLines { @(Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String 'favicon-probe') }
function Popups {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' })
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if ($Phase -eq 'main') {
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
  StartKrunk; StopKrunk
  Log "[$P] seed: $(python "$K\vm-0110-seed.py" $profile)"
}
$t0 = Get-Date
StartKrunk
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$wDip = ($c.Right - $c.Left) / $c.Scale
$barY = Y 90
$rowY0 = Y 126

if ($Phase -eq 'restart') {
  Shot "$P-1-bar-icons-kept"
  Start-Sleep 40
  $lines = ProbeLines
  Log "[$P] favicon-probe lines in this session (40 s after the start): $($lines.Count)"
  $lines | ForEach-Object { Log "[$P] $_" }
  StopKrunk
  Log "[$P] done after $([int]((Get-Date) - $t0).TotalSeconds) s"
  return
}

SafeGo 'http://localhost:8094/words.html' 2500
# ---- 1: folder dropdowns open with their own content (no frame of the other folder).
for ($k = 0; $k -lt 3; $k++) {
  Click (X 30) $barY 70
  Shot "$P-1-news-$k"
  Keys '{ESC}' 250
  Click (X 100) $barY 70
  Shot "$P-1-dev-$k"
  Keys '{ESC}' 250
}
# ---- 2: the overflow menu: End, Up = "Later"; Enter opens it in place with "‹ Back".
Click (X ($wDip - 22)) $barY 900
Shot "$P-2-overflow"
Keys '{END}' 150; Keys '{UP}' 150; Keys '{ENTER}' 900
Shot "$P-2-later-with-back"
Keys '{END}' 150; Keys '{ENTER}' 900
Shot "$P-2-inner"
Keys '{BACKSPACE}' 900
Shot "$P-2-back-to-later"
# ---- 3: middle-click Wikipedia (row 2, after Back): a tab behind, the menu stays open.
MiddleClick (X 60) ($rowY0 + [int](32 * $c.Scale)) 1500
Log "[$P] after the middle-click: popups visible $(@(Popups).Count), title '$([TW]::Txt($A))'"
Shot "$P-3-middle-click-keeps-menu"
Keys '{ESC}' 500
Shot "$P-3-new-tab-behind"
# ---- 4: the favicon probe (30 s after the start): icons on the bar for never-visited sites.
$wait = 42 - ((Get-Date) - $t0).TotalSeconds
if ($wait -gt 0) { Start-Sleep -Seconds ([int]$wait) }
Shot "$P-4-bar-icons"
ProbeLines | ForEach-Object { Log "[$P] $_" }
# ---- 15/16: source viewer and search for selected text, on words.html.
Focus $A | Out-Null
SafeKeys '^1' 800
SafeKeys '^u' 2500
Shot "$P-15-source"
Click (X ($wDip - 175)) (Y 92) 1200     # "Beautify" in the viewer's bar (left of Wrap lines)
Shot "$P-15-beautified"
Log "[$P] notepad processes: $(@(Get-Process notepad -ErrorAction SilentlyContinue).Count)"
SafeKeys '^1' 800
DoubleClick (X 160) (Y 210)              # "krunk" in the first paragraph
Start-Sleep -Milliseconds 400
RightClick (X 160) (Y 210) 900
Shot "$P-16-selection-menu"
Keys '{ESC}' 400
# ---- 8: 60 tabs: overflow UI, tab list, middle-click without autoscroll, wheel.
Focus $A | Out-Null
for ($k = 0; $k -lt 57; $k++) { Keys '^t' 120 }
Start-Sleep 2
Shot "$P-8-60-tabs"
$lb = $c.Right - [int](225 * $c.Scale)   # "▾" sits right of "+" (the caption buttons take 138 DIP)
Log "[$P] tabs: title '$([TW]::Txt($A))'"
Click (X 640) (Y 18) 600                 # a tab in the middle of the scrolled row
MiddleClick (X 640) (Y 18) 900
Shot "$P-8-middle-click-closed"
Wheel (X 640) (Y 18) 5
Start-Sleep -Milliseconds 600
Shot "$P-8-wheel-scrolled"
KrunkWindows $P
StopKrunk
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR|popup: shown without|source:|app identity' | Select-Object -Last 15) -join ' ## ')"
Log "[$P] done after $([int]((Get-Date) - $t0).TotalSeconds) s"
