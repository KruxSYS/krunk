# Krunk DPI checks that are not part of the per-scale matrix (scripts/vm-dpi-shots.ps1):
#   vm-dpi-extra.ps1 -Phase narrow [-Percent 175]   the window at its minimum size and at a
#                                                    quarter of the screen: the toolbar keeps ⋮
#   vm-dpi-extra.ps1 -Phase text                     Windows' "Text size" at 150 % (Accessibility),
#                                                    display scale 100 %: Krunk's own UI stays as
#                                                    it is; restored afterwards
# 1920x1080 for both. Shots "extra-<phase>-NN-name.png" and log lines in run.log; the display
# goes back to 1280x800 @ 100 % at the end.
param([string]$Phase = 'narrow', [int]$Percent = 175, [string]$App = 'app')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$Home0 = 'C:\pipeline\krunk'
$W = 'http://127.0.0.1:8094'
$Py = 'C:\Program Files\Python312\python.exe'
$T = "extra-$Phase"
Remove-Item "$Shots\$T-*" -Force -ErrorAction SilentlyContinue
$script:n = 0
function Snap([string]$name) { $script:n++; Shot ("$T-{0:d2}-$name" -f $script:n); Log "[$T] shot $name" }
function Note([string]$s) { Log "[$T] $s" }
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 1
}
function StartKrunk([string]$prof) {
  $env:LOCALAPPDATA = $prof
  Start-Process -FilePath "$Home0\$App\Krunk.exe" -WorkingDirectory "$Home0\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $script:A = MainWindow
  for ($i = 0; $i -lt 40 -and [TW]::Txt($script:A) -notmatch 'Krunk'; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  for ($i = 0; $i -lt 4 -and -not (Focus $script:A); $i++) { Start-Sleep 1 }
}
function Place([double]$wDip, [double]$hDip) {
  $s = (ClientRect $script:A).Scale
  [U]::MoveWindow($script:A, [int](8 * $s), [int](8 * $s), [int]($wDip * $s), [int]($hDip * $s), $true) | Out-Null; Start-Sleep 2
  $r = WindowRect $script:A; $c = ClientRect $script:A
  Note "window asked $wDip x $hDip DIP: got $([Math]::Round(($r.Right - $r.Left) / $s, 1)) x $([Math]::Round(($r.Bottom - $r.Top) / $s, 1)) DIP, client $($c.Right - $c.Left) px at scale $s, title '$([TW]::Txt($script:A))'"
  $script:c = $c
}
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class XW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  public static List<IntPtr> Children(IntPtr p) { var l = new List<IntPtr>(); EnumChildWindows(p, (h, x) => { l.Add(h); return true; }, IntPtr.Zero); return l; }
}
'@
# The frame's child views and any popup window of Krunk's, in pixels.
function Rects([string]$label) {
  $cc = ClientRect $script:A
  Note "$label client=$($cc.Right - $cc.Left)x$($cc.Bottom - $cc.Top) window-scale=$($cc.Scale) title='$([TW]::Txt($script:A))'"
  foreach ($kid in [XW]::Children($script:A)) {
    if ([XW]::GetParent($kid) -ne $script:A -or -not [U]::IsWindowVisible($kid)) { continue }
    $r = WindowRect $kid
    Note "$label child rect=$($r.Left - $cc.Left),$($r.Top - $cc.Top),$($r.Right - $cc.Left),$($r.Bottom - $cc.Top)"
  }
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  foreach ($h in [TW]::All()) {
    if ($pids -notcontains [TW]::Pid($h) -or -not [TW]::IsWindowVisible($h) -or $h -eq $script:A) { continue }
    if ([TW]::Cls($h) -ne 'KrunkPopup') { continue }
    $r = WindowRect $h
    Note "$label popup rect=$($r.Left - $cc.Left),$($r.Top - $cc.Top),$($r.Right - $cc.Left),$($r.Bottom - $cc.Top) (relative to the client area) size=$($r.Right - $r.Left)x$($r.Bottom - $r.Top)"
  }
}
# The profile of the matrix (bookmarks bar, AI on), made anew.
$patch = '{"bookmarksBar": "always", "checkUpdatesAutomatically": false, "theme": "light", "startup": "newTab", "ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"}}'
$prof = "$Home0\pdpix"
$TextKey = 'HKCU:\Software\Microsoft\Accessibility'
try {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-File', "$Home0\serve.ps1", '-PrefixList', 'http://*:8094/'
    Start-Sleep 2
  }
  $pct = if ($Phase -eq 'text') { 100 } else { $Percent }
  & "$Home0\vm-display.ps1" -Width 1920 -Height 1080 -Percent $pct | ForEach-Object { Note "display: $_" }
  StopKrunk
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $prof | Out-Null
  StartKrunk $prof; StopKrunk
  [IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
  Note "setkv: $(& $Py "$Home0\setkv.py" $prof "@$prof\patch.json" 2>&1)"
  Note "seed: $(& $Py "$Home0\dpi-seed.py" $prof 2>&1)"
  if ($Phase -eq 'text') {
    New-Item -Path $TextKey -Force | Out-Null
    Set-ItemProperty -Path $TextKey -Name TextScaleFactor -Value 150 -Type DWord
    Note "text size: TextScaleFactor = $((Get-ItemProperty $TextKey).TextScaleFactor)"
  }
  StartKrunk $prof
  if ($Phase -eq 'narrow') {
    foreach ($size in @(@(480, 320), @(548, 569), @(640, 400), @(960, 560))) {
      Place $size[0] $size[1]
      SafeGo "$W/dpi.html" 2500
      $c = ClientRect $script:A
      Snap "w$($size[0])"
      # The ⋮ button: 22 DIP from the right edge, in the toolbar row.
      Click ([int]($c.Right - 22 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 1200
      Snap "w$($size[0])-menu"
      Keys '{ESC}' 500
    }
    # Zoomed in (the zoom chip takes room in the address field) at the minimum size.
    Place 480 320
    SafeKeys '^{ADD}' 600; SafeKeys '^{ADD}' 600
    Snap 'w480-zoomed'
    SafeKeys '^0' 600
  } else {
    Place 960 560
    SafeGo "$W/dpi.html" 2500
    $c = ClientRect $script:A
    Snap 'main'; Rects 'main'
    # The page tells what the engine draws at ("dpr=" in its title): the click points follow it.
    $dpr = 1.0
    if ([TW]::Txt($script:A) -match 'dpr=([0-9.]+)') { $dpr = [double]$Matches[1] }
    Note "the page's devicePixelRatio: $dpr"
    Click ([int]($c.Right - 22 * $dpr)) ([int]($c.Top + 56 * $dpr)) 1200
    Snap 'menu'; Rects 'menu'
    Keys '{ESC}' 500
    $tp = @([int]($c.Left + 48 * $dpr), [int]($c.Top + 18 * $dpr))
    RightClick $tp[0] $tp[1] 1000
    Snap 'tabmenu'; Rects 'tabmenu'
    Keys '{ESC}' 500
    SafeGo 'krunk://settings' 3000
    Snap 'settings'
    SafeKeys '^+ ' 2000
    Snap 'ai'; Rects 'ai'
    SafeKeys '^+ ' 800
    [U]::ShowWindow($script:A, 3) | Out-Null; Start-Sleep 2
    SafeGo "$W/dpi.html" 2500
    Snap 'max'; Rects 'max'
    $c = ClientRect $script:A
    Click ([int]($c.Right - 22 * $dpr)) ([int]($c.Top + 56 * $dpr)) 1200
    Snap 'max-menu'; Rects 'max-menu'
    Keys '{ESC}' 500
  }
  Get-Content "$prof\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String 'WARN|ERROR|panic' | Select-Object -First 20 | ForEach-Object { Note "log: $($_.Line)" }
} catch {
  Note "ERROR $_"
} finally {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  if ($Phase -eq 'text') {
    Remove-ItemProperty -Path $TextKey -Name TextScaleFactor -ErrorAction SilentlyContinue
    Note "text size: restored ($((Get-ItemProperty $TextKey -ErrorAction SilentlyContinue).TextScaleFactor))"
  }
  try { & "$Home0\vm-display.ps1" -Restore | ForEach-Object { Note "display: $_" } } catch { Note "display: RESTORE FAILED $_" }
}
