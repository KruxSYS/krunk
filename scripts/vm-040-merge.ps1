# Krunk 0.4.0: merging a tab into another window must leave it (and that window) in front.
# Overlapping windows, unlike vm-030-windows.ps1. Screenshots while the tab hovers over the
# target strip ("-hover") and after the drop ("-dropped"); the z-order (top first) is logged
# each time. Fresh profile; needs serve.ps1 on localhost:8092 (started here). Prefix "merge-".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'merge'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class W {
  public struct PT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(PT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, out int v, uint c);
}
'@
$track = 0; [W]::SystemParametersInfo(0x1000, 0, [ref]$track, 0) | Out-Null
Log "[setup] active window tracking (X-Mouse): $track"
# Press at 1, move through m to 2, screenshot while still holding, then release.
function DragHold([string]$name, [int]$x1, [int]$y1, [int]$xm, [int]$ym, [int]$x2, [int]$y2) {
  [U]::SetCursorPos($x1, $y1) | Out-Null; AbsMove $x1 $y1; Start-Sleep -Milliseconds 150
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 300
  foreach ($seg in @(@($x1, $y1, $xm, $ym), @($xm, $ym, $x2, $y2))) {
    for ($i = 1; $i -le 15; $i++) { AbsMove ($seg[0] + ($seg[2] - $seg[0]) * $i / 15) ($seg[1] + ($seg[3] - $seg[1]) * $i / 15); Start-Sleep -Milliseconds 30 }
  }
  Start-Sleep -Milliseconds 600
  $under = [W]::GetAncestor([W]::WindowFromPoint([W+PT]@{ X = $x2; Y = $y2 }), 2)
  Log "[$name] under the cursor: '$([TW]::Txt($under))'; foreground: '$([TW]::Txt([U]::GetForegroundWindow()))'"
  Shot "$P-$name-hover"; KrunkWindows "$name-hover"
  [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 1500
  Shot "$P-$name-dropped"; KrunkWindows "$name-dropped"
  Log "[$name] foreground after drop: '$([TW]::Txt([U]::GetForegroundWindow()))'"
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}
$profile = 'C:\pipeline\krunk\p040m'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
# A at the top left; B overlaps A's lower right and is in front. Both strips stay visible
# (B starts below A's strip; B's tabs start right of A).
[U]::MoveWindow($A, 0, 0, 520, 460, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
SafeGo 'https://example.com/' 2500
SafeKeys '^n' 2500
$B = [U]::GetForegroundWindow()
[U]::MoveWindow($B, 480, 300, 760, 450, $true) | Out-Null; Start-Sleep 1
SafeGo 'krunk://about' 2000
SafeKeys '^t' 1200; SafeGo 'http://localhost:8092/move.html?merge=1' 2500
KrunkWindows 'setup'

# (1) Tear B's second tab out and drop it on A's strip. A is behind B: after the drop, A
#     (with the tab) must be in front.
$tb = TabPoint $B 1 2
$ta = TabPoint $A 0 1
DragHold 's1' $tb[0] $tb[1] $tb[0] ($tb[1] + 160) ($ta[0] + 160) $ta[1]

# (2) Bring B to the front again, then drag its only tab (the whole window) onto A's strip:
#     B merges into A and closes; A must be in front.
Focus $B | Out-Null; Start-Sleep -Milliseconds 500
KrunkWindows 's2-before'
$tb = TabPoint $B 0 1
$ta = TabPoint $A 0 2
DragHold 's2' $tb[0] $tb[1] ($tb[0] - 100) ($tb[1] + 100) ($ta[0] + 330) $ta[1]

# (3) Tab menu "Move to new window" (A now has three tabs): the new window must end up active
#     and stay so (no activation tug-of-war between the two windows).
$ta = TabPoint $A 0 3
RightClick $ta[0] $ta[1] 1200
Click ($ta[0] + 60) ($ta[1] + 81) 3000
Shot "$P-s3-moved"; KrunkWindows 's3'
Log "[s3] foreground 3 s after Move to new window: '$([TW]::Txt([U]::GetForegroundWindow()))'"

foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'tab |window|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
