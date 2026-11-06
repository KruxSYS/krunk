# Krunk 0.5.0 milestone 5: the native overlays and the DevTools strip, at -Percent 100 or 125.
# (a) The link-status label: hovering a long link shows its address, elided, bottom-left of the
#     page; a link in that corner moves it to the bottom-right; plain text clears it; F11 full
#     screen shows none. (b) The "Press Esc" hint: a pill that fades in on each of the first three
#     pointer locks of a tab and not on the fourth, and again in a new tab; white pixels in its
#     area are counted ("[hint] ..."). (c) The DevTools strip: its × closes DevTools (right and
#     bottom dock), its dock button moves it. (d) With Windows' animations off, the hint shows at
#     full opacity at once. Fresh profile; needs serve.ps1 on :8090 and :8094. Screenshots "ov<P>-*".
param([int]$Percent = 100)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "ov$Percent"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class SP {
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, IntPtr v, uint c);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint b, out int v, uint c);
  public static int GetAnim() { int v; SystemParametersInfo(0x1042, 0, out v, 0); return v; }
  public static void SetAnim(bool on) { SystemParametersInfo(0x1043, 0, (IntPtr)(on ? 1 : 0), 3); }
}
'@
Add-Type -AssemblyName System.Drawing
# Counts near-white pixels in a screen rectangle (the hint's text), and saves the rectangle.
function Whites([string]$name, [int]$x, [int]$y, [int]$w, [int]$h) {
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size); $g.Dispose()
  $n = 0
  for ($yy = 0; $yy -lt $h; $yy += 2) { for ($xx = 0; $xx -lt $w; $xx += 2) { $c = $bmp.GetPixel($xx, $yy); if ($c.R -gt 200 -and $c.G -gt 200 -and $c.B -gt 200) { $n++ } } }
  $bmp.Save("$Shots\$P-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  $n
}
foreach ($port in 8090, 8094) {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',"http://*:$port/"
    Start-Sleep 2
  }
}
& powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent $Percent | ForEach-Object { Log "[$P] dpi: $_" }
Start-Sleep 2
$animBefore = [SP]::GetAnim()
function StartKrunk([string]$profile) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
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
function Quit { foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }; Start-Sleep 3 }
$profile = 'C:\pipeline\krunk\p050o'
try {
  [SP]::SetAnim($true)
  $A = StartKrunk $profile
  $c = ClientRect $A
  $s = $c.Scale
  Log "[$P] window scale $s"
  function X([double]$dip) { [int]($c.Left + $dip * $s) }
  function Y([double]$dip) { [int]($c.Top + $dip * $s) }
  $pageTop = 104   # shell 76 + bookmarks bar 28 (DIP)

  # (a) Link hover status.
  SafeGo 'http://localhost:8094/links.html' 3000
  MoveTo (X 100) (Y ($pageTop + 55)); Start-Sleep -Milliseconds 700
  Shot "$P-1-status-bottom-left"
  MoveTo (X 100) (Y ($pageTop + 125)); Start-Sleep -Milliseconds 700
  Shot "$P-2-status-cleared"
  $pageBottom = [int](($c.Bottom - $c.Top) / $s)
  MoveTo (X 60) (Y ($pageBottom - 20)); Start-Sleep -Milliseconds 900
  Shot "$P-3-status-moved-right"
  MoveTo (X 100) (Y ($pageTop + 125)); Start-Sleep -Milliseconds 500
  SafeKeys '{F11}' 2500
  MoveTo (X 100) (Y 55); Start-Sleep -Milliseconds 900
  Shot "$P-4-fullscreen-no-status"
  SafeKeys '{F11}' 2000

  # (c) DevTools strip (right dock, then bottom).
  SafeGo 'https://example.com/' 3000
  SafeKeys '{F12}' 4000
  Shot "$P-5-devtools-right-strip"
  $closeX = [int]($c.Right - 14 * $s); $stripY = Y ($pageTop + 12)
  MoveTo $closeX $stripY; Start-Sleep -Milliseconds 600
  Shot "$P-6-strip-close-hover"
  Click $closeX $stripY 2000
  Shot "$P-7-devtools-closed-by-strip"
  SafeKeys '{F12}' 4000
  $dockX = [int]($c.Right - 38 * $s)
  Click $dockX $stripY 2500
  Shot "$P-8-devtools-bottom-strip"
  # Bottom: the strip is 5 DIP under the page, whose height is the content minus 5 minus 320.
  $contentH = $pageBottom - $pageTop
  $bottomStripY = Y ($pageTop + ($contentH - 5 - 320) + 5 + 12)
  MoveTo $closeX $bottomStripY; Start-Sleep -Milliseconds 600
  Shot "$P-9-bottom-strip-close-hover"
  Click $closeX $bottomStripY 2000
  Shot "$P-10-devtools-closed-bottom"
  # Back to the right dock for the next runs (the setting is saved).
  SafeKeys '{F12}' 3000; Click $dockX $bottomStripY 1500; Click $closeX $stripY 1500

  # (b) The hint: four locks in one tab, then one in a new tab.
  SafeGo 'http://localhost:8090/pointer-lock.html' 3000
  $hintX = [int](($c.Left + $c.Right) / 2 - 150 * $s); $hintY = Y ($pageTop + 24 + 4); $hintW = [int](300 * $s); $hintH = [int](30 * $s)
  $stageX = [int](($c.Left + $c.Right) / 2); $stageY = Y ($pageTop + 150)
  for ($i = 1; $i -le 4; $i++) {
    Click $stageX $stageY 450
    $n = Whites "hint-lock$i" $hintX $hintY $hintW $hintH
    Log "[hint] lock $i in tab 1: white pixels in the hint area = $n (expected > 0 for 1-3, 0 for 4)"
    Keys '{ESC}' 3500
  }
  SafeKeys '^t' 1500; SafeGo 'http://localhost:8090/pointer-lock.html' 3000
  Click $stageX $stageY 450
  $n = Whites 'hint-newtab' $hintX $hintY $hintW $hintH
  Log "[hint] lock 1 in tab 2: white pixels in the hint area = $n (expected > 0)"
  Keys '{ESC}' 3500
  # The fade: frames right after a lock in a third tab (reduced motion compared below).
  SafeKeys '^t' 1500; SafeGo 'http://localhost:8090/pointer-lock.html' 3000
  Click $stageX $stageY 40
  $fade = @(); for ($i = 0; $i -lt 6; $i++) { $fade += Whites "fade-anim-$i" $hintX $hintY $hintW $hintH; Start-Sleep -Milliseconds 25 }
  Log "[hint] animations on, white pixels over the first ~150 ms: $($fade -join ', ')"
  Keys '{ESC}' 3500
  Quit

  # (d) Reduced motion: the hint is fully there at once.
  [SP]::SetAnim($false)
  $A = StartKrunk $profile
  SafeGo 'http://localhost:8090/pointer-lock.html' 3000
  Click $stageX $stageY 40
  $fade = @(); for ($i = 0; $i -lt 6; $i++) { $fade += Whites "fade-reduced-$i" $hintX $hintY $hintW $hintH; Start-Sleep -Milliseconds 25 }
  Log "[hint] animations off, white pixels over the first ~150 ms: $($fade -join ', ')"
  Keys '{ESC}' 3500
  Quit
} finally {
  [SP]::SetAnim($animBefore -ne 0)
  if ($Percent -ne 100) { & powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent 100 | ForEach-Object { Log "[$P] dpi restored: $_" } }
}
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'hint|DevTools strip|pointer locked|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
