# Krunk 0.5.0 milestone 4: smooth tab reordering. Four tabs; the first is dragged slowly to
# the right end of the strip while the tab row is captured every ~40 ms ("reorder-drag-NN"):
# the dragged tab follows the pointer and the others slide aside; after the release the
# tab settles into its slot ("reorder-settle-NN"). Then the same with Windows' animations
# off (SPI_SETCLIENTAREAANIMATION, which Chromium maps to prefers-reduced-motion): the
# others do not slide, the drop re-lays the strip out at once. A tear-off (drag below the
# strip) must still make a new window. Fresh profile. Screenshots "reorder-*".
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'reorder'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class SP {
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, IntPtr v, uint c);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, out int v, uint c);
  public static int GetAnim() { int v; SystemParametersInfo(0x1042, 0, out v, 0); return v; }
  public static void SetAnim(bool on) { SystemParametersInfo(0x1043, 0, (IntPtr)(on ? 1 : 0), 3); }
}
'@
function Frame([string]$name, [int]$i, [int]$ms) {
  $bmp = New-Object System.Drawing.Bitmap 1280, 40
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size); $g.Dispose()
  $bmp.Save("$Shots\$P-$name-$('{0:D2}' -f $i)-${ms}ms.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
# Press at (x1,y), move to (x2,y) in $steps with a frame after each step, hold, then release
# and capture the settle.
function DragBurst([string]$name, [int]$x1, [int]$x2, [int]$y, [int]$steps) {
  [U]::SetCursorPos($x1, $y) | Out-Null; AbsMove $x1 $y; Start-Sleep -Milliseconds 200
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 300
  $t0 = Get-Date
  for ($i = 1; $i -le $steps; $i++) {
    AbsMove ($x1 + ($x2 - $x1) * $i / $steps) $y
    Start-Sleep -Milliseconds 25
    Frame "$name-drag" $i ([int]((Get-Date) - $t0).TotalMilliseconds)
  }
  Start-Sleep -Milliseconds 250
  Frame "$name-drag" ($steps + 1) ([int]((Get-Date) - $t0).TotalMilliseconds)
  [SI]::Mouse(4, 0, 0) | Out-Null
  $t1 = Get-Date
  for ($i = 0; $i -lt 10; $i++) { Frame "$name-settle" $i ([int]((Get-Date) - $t1).TotalMilliseconds); Start-Sleep -Milliseconds 30 }
  Start-Sleep -Milliseconds 400
}
function StartKrunk([string]$profile, [bool]$fresh) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  if ($fresh) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
  $A
}
function Quit {
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
}

$animBefore = [SP]::GetAnim()
Log "[setup] client area animation before: $animBefore"
$profile = 'C:\pipeline\krunk\p050r'
try {
  # (a) Animations on: four tabs, the first dragged to the end.
  [SP]::SetAnim($true)
  $A = StartKrunk $profile $true
  $c = ClientRect $A
  SafeGo 'https://example.com/' 2500
  SafeKeys '^t' 800; SafeGo 'https://example.org/' 2500
  SafeKeys '^t' 800; SafeGo 'https://example.net/' 2500
  SafeKeys '^t' 800; SafeGo 'https://www.iana.org/' 3000
  Shot "$P-1-four-tabs"
  $tabY = [int]($c.Top + 18 * $c.Scale)
  DragBurst 'anim' ([int]($c.Left + 60 * $c.Scale)) ([int]($c.Left + 900 * $c.Scale)) $tabY 24
  Shot "$P-2-after-drop"
  # (b) Tear-off: the second tab dragged below the strip makes a new window.
  Drag ([int]($c.Left + 300 * $c.Scale)) $tabY ([int]($c.Left + 400 * $c.Scale)) ([int]($c.Top + 300 * $c.Scale)) 20
  Start-Sleep 2
  KrunkWindows "$P-tearoff"
  Shot "$P-3-torn-off"
  Quit
  # (c) Animations off (reduced motion): the same drag.
  [SP]::SetAnim($false)
  Log "[setup] client area animation set off: $([SP]::GetAnim())"
  $A = StartKrunk $profile $true
  $c = ClientRect $A
  SafeGo 'https://example.com/' 2500
  SafeKeys '^t' 800; SafeGo 'https://example.org/' 2500
  SafeKeys '^t' 800; SafeGo 'https://example.net/' 2500
  SafeKeys '^t' 800; SafeGo 'https://www.iana.org/' 3000
  DragBurst 'reduced' ([int]($c.Left + 60 * $c.Scale)) ([int]($c.Left + 900 * $c.Scale)) $tabY 24
  Shot "$P-4-reduced-after-drop"
  Quit
} finally {
  [SP]::SetAnim($animBefore -ne 0)
  Log "[end] client area animation restored: $([SP]::GetAnim())"
}
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'tab .*drag|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
