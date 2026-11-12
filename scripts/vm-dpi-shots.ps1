# Krunk DPI check on the test VM: screenshots and numbers of the main surfaces at one display
# mode and scale (scripts/vm-dpi-run.sh runs it for a whole matrix and collects the results).
#   vm-dpi-shots.ps1 -Width 1920 -Height 1080 -Percent 150 [-Set core|all]
#   vm-dpi-shots.ps1 -Width 1920 -Height 1080 -Percent 150 -Runtime
#       starts at 100 %, changes to -Percent while Krunk runs (a menu open), back to 100 %,
#       then restarts Krunk at -Percent (saved window bounds from another scale)
# The window gets a fixed size in DIP (960x560 at 8,8), so every control has the same DIP
# position at every scale and every click point is DIP x scale: nothing is hard-coded in pixels.
# Per surface: a screenshot "<mode>-<percent>-NN-name.png". Per state (normal, maximised, full
# screen, ...) log lines with the window and client rectangle, the work area, every visible child
# window, WM_NCHITTEST at named points, and what the page sees (devicePixelRatio, viewport), read
# from the window title of dpi.html. The display is restored to 1280x800 @ 100 % at the end, also
# after a failure.
param([int]$Width = 1280, [int]$Height = 800, [int]$Percent = 100, [string]$Set = 'all', [switch]$Runtime, [string]$App = 'app')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$W = 'http://127.0.0.1:8094'
$Py = 'C:\Program Files\Python312\python.exe'
$T = "${Width}x$Height-$Percent"
if ($Runtime) { $T = "$T-rt" }
Remove-Item "$Shots\$T-*" -Force -ErrorAction SilentlyContinue
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class DW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct MI { public int cb; public R mon; public R work; public uint flags; }
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr res);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
  [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr m, ref MI i);
  public static List<IntPtr> Children(IntPtr p) { var l = new List<IntPtr>(); EnumChildWindows(p, (h, x) => { l.Add(h); return true; }, IntPtr.Zero); return l; }
  // WM_NCHITTEST at a screen point; -99 when the window does not answer within 2 s.
  public static int Hit(IntPtr h, int x, int y) {
    IntPtr res; IntPtr lp = (IntPtr)(((y & 0xFFFF) << 16) | (x & 0xFFFF));
    return SendMessageTimeout(h, 0x84, IntPtr.Zero, lp, 2, 2000, out res) == IntPtr.Zero ? -99 : (int)res;
  }
  public static R Work(IntPtr h) { var i = new MI(); i.cb = Marshal.SizeOf(typeof(MI)); GetMonitorInfo(MonitorFromWindow(h, 2), ref i); return i.work; }
}
'@
$script:n = 0
function Snap([string]$name) { $script:n++; Shot ("$T-{0:d2}-$name" -f $script:n); Log "[$T] shot $name" }
function Note([string]$s) { Log "[$T] $s" }
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function XR([double]$dip) { [int]($c.Right - $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# Page coordinates (CSS px of dpi.html) to the screen: the page starts under the shell strip
# (76 DIP) and the bookmarks bar (28 DIP).
function PX([double]$css) { X $css }
function PY([double]$css) { Y (104 + $css) }
function Display([int]$p) {
  try { & "$K\vm-display.ps1" -Width $Width -Height $Height -Percent $p | ForEach-Object { Note "display: $_" } }
  catch { Note "display: FAILED $_"; throw }
}
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-Process Krunk -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
  for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 1
}
# Starts Krunk and waits until its first tab has a title. $place: put the window at the fixed
# DIP rectangle (otherwise it keeps what Krunk chose, which is itself under test).
function StartKrunk([string]$prof, [bool]$place = $true) {
  $env:LOCALAPPDATA = $prof
  Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $script:A = MainWindow
  for ($i = 0; $i -lt 40 -and [TW]::Txt($script:A) -notmatch 'Krunk'; $i++) { Start-Sleep -Milliseconds 500 }
  # Ready when the shell has reported its layout: the close button answers as one (HTCLOSE).
  for ($i = 0; $i -lt 60; $i++) {
    $cc = ClientRect $script:A
    if ([DW]::Hit($script:A, [int]($cc.Right - 23 * $cc.Scale), [int]($cc.Top + 18 * $cc.Scale)) -eq 20) { break }
    Start-Sleep -Milliseconds 500
  }
  Note "started: ready after $($i * 0.5) s more"
  Start-Sleep 3
  $script:c = ClientRect $script:A
  if ($place) {
    $s = $script:c.Scale
    [U]::MoveWindow($script:A, [int](8 * $s), [int](8 * $s), [int](960 * $s), [int](560 * $s), $true) | Out-Null; Start-Sleep 1
    $script:c = ClientRect $script:A
  }
  for ($i = 0; $i -lt 4 -and -not (Focus $script:A); $i++) { Start-Sleep 1 }
  if ([U]::GetForegroundWindow() -ne $script:A) { Note 'could not focus'; throw 'no focus' }
}
function Fresh([string]$prof, [string]$patch) {
  StopKrunk
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $prof | Out-Null
  StartKrunk $prof $false
  Geo 'first-start'
  Snap 'first-start'
  StopKrunk
  [IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
  $set = "$(& $Py "$K\setkv.py" $prof "@$prof\patch.json" 2>&1)"
  if ($set -match 'Traceback') { Start-Sleep 5; $set = "$(& $Py "$K\setkv.py" $prof "@$prof\patch.json" 2>&1)" }
  Note "setkv: $set"
  if ($set -match 'Traceback') { throw 'the profile could not be prepared' }
  Note "seed: $(& $Py "$K\dpi-seed.py" $prof 2>&1)"
}
# Loads a page in the active tab. $expect: text its window title must show (typed keys can get
# lost while the VM is busy: checked, and typed again up to three times).
function Page([string]$url, [int]$wait = 2500, [string]$expect = '') {
  if ($expect -eq '' -and $url -like '*dpi.html') { $expect = 'DPI dpr=' }
  foreach ($try in 1..3) {
    Focus $script:A | Out-Null; SafeGo $url $wait
    if ($expect -eq '') { return }
    for ($i = 0; $i -lt 10 -and [TW]::Txt($script:A) -notlike "*$expect*"; $i++) { Start-Sleep -Milliseconds 500 }
    if ([TW]::Txt($script:A) -like "*$expect*") { return }
    Note "page ${url}: title is '$([TW]::Txt($script:A))' after try $try"
    Esc
  }
  throw "page $url did not load"
}
function Esc([int]$wait = 500) { Keys '{ESC}' $wait }
# Numbers for the current state of window $h: rectangles, children, hit tests, the page's view.
function Geo([string]$label, [IntPtr]$h = [IntPtr]::Zero) {
  if ($h -eq [IntPtr]::Zero) { $h = $script:A }
  $w = WindowRect $h; $cc = ClientRect $h; $wk = [DW]::Work($h)
  if ($h -eq $script:A) { $script:c = $cc }
  Note "geo $label window=$($w.Left),$($w.Top),$($w.Right),$($w.Bottom) client=$($cc.Left),$($cc.Top),$($cc.Right),$($cc.Bottom) work=$($wk.Left),$($wk.Top),$($wk.Right),$($wk.Bottom) scale=$($cc.Scale) zoomed=$([U]::IsZoomed($h)) title='$([TW]::Txt($h))'"
  foreach ($kid in [DW]::Children($h)) {
    if ([DW]::GetParent($kid) -ne $h -or -not [U]::IsWindowVisible($kid)) { continue }
    $r = WindowRect $kid
    Note "geo $label child class=$([TW]::Cls($kid)) rect=$($r.Left - $cc.Left),$($r.Top - $cc.Top),$($r.Right - $cc.Left),$($r.Bottom - $cc.Top) of $($cc.Right - $cc.Left)x$($cc.Bottom - $cc.Top)"
  }
  $s = $cc.Scale; $cw = ($cc.Right - $cc.Left) / $s
  # name, x DIP (negative: from the right edge), y DIP
  $pts = @(@('close', -23, 18), @('max', -69, 18), @('min', -115, 18), @('drag', -200, 18), @('top-band', -200, 1),
           @('tab', 48, 18), @('tab-close', 246, 21), @('newtab', 280, 20), @('toolbar', ($cw / 2), 56), @('page', ($cw / 2), 300),
           @('left-edge', 1, 300), @('bottom-edge', ($cw / 2), -1))
  $out = @()
  foreach ($p in $pts) {
    $x = if ($p[1] -lt 0) { $cc.Right + $p[1] * $s } else { $cc.Left + $p[1] * $s }
    $y = if ($p[2] -lt 0) { $cc.Bottom + $p[2] * $s } else { $cc.Top + $p[2] * $s }
    $out += "$($p[0])=$([DW]::Hit($h, [int]$x, [int]$y))"
  }
  Note "geo $label hit $($out -join ' ')"
}
# The popup window that is open now (class KrunkPopup), against the frame and the work area.
function PopupGeo([string]$label) {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $wk = [DW]::Work($script:A)
  foreach ($h in [TW]::All()) {
    if ($pids -notcontains [TW]::Pid($h) -or -not [TW]::IsWindowVisible($h) -or $h -eq $script:A) { continue }
    $cls = [TW]::Cls($h); if ($cls -eq 'KrunkWindow' -or $cls -eq 'SysShadow') { continue }
    $r = WindowRect $h
    $in = ($r.Left -ge $wk.Left -and $r.Top -ge $wk.Top -and $r.Right -le $wk.Right -and $r.Bottom -le $wk.Bottom)
    Note "popup $label class=$cls rect=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom) dip=$([Math]::Round(($r.Right - $r.Left) / $c.Scale, 1))x$([Math]::Round(($r.Bottom - $r.Top) / $c.Scale, 1)) rel=$([Math]::Round(($r.Left - $c.Left) / $c.Scale, 1)),$([Math]::Round(($r.Top - $c.Top) / $c.Scale, 1)) inside-work-area=$in"
  }
}
function Surface([string]$name, [scriptblock]$open, [scriptblock]$close = { Esc }) {
  try {
    Focus $script:A | Out-Null
    & $open
    Snap $name
    PopupGeo $name
    & $close
  } catch { Note "surface $name FAILED: $_"; try { Esc } catch {} }
}
function Serve {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-File', "$K\serve.ps1", '-PrefixList', 'http://*:8094/'
    Start-Sleep 2
  }
  if (-not (Get-NetTCPConnection -LocalPort 8110 -State Listen -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath $Py -ArgumentList "$K\authserve.py", '8110' -WindowStyle Hidden
    Start-Sleep 2
  }
}
$patch = '{"bookmarksBar": "always", "checkUpdatesAutomatically": false, "theme": "light", "startup": "newTab", "ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"}}'
$prof = "$K\pdpi"

try {
  Serve
  if ($Runtime) {
    # --- scale changed while Krunk runs ---
    Display 100
    Fresh $prof $patch
    StartKrunk $prof
    Page "$W/dpi.html"
    Geo 'rt-before'; Snap 'rt-before'
    Click (XR 22) (Y 56) 1200
    Snap 'rt-menu-before'; PopupGeo 'rt-menu-before'
    Display $Percent
    Start-Sleep 3
    Snap 'rt-changed-menu-was-open'; PopupGeo 'rt-changed'
    Esc
    Geo 'rt-changed'; Snap 'rt-changed'
    Surface 'rt-menu' { Click (XR 22) (Y 56) 1200 }
    Surface 'rt-tabmenu' { $tp = TabPoint $script:A 0; RightClick $tp[0] $tp[1] 1000 }
    Surface 'rt-pagemenu' { RightClick (PX 200) (PY 205) 1000 }
    Surface 'rt-ai' { SafeKeys '^+ ' 2000; Geo 'rt-ai' } { SafeKeys '^+ ' 800 }
    [U]::ShowWindow($script:A, 3) | Out-Null; Start-Sleep 2
    Geo 'rt-max'; Snap 'rt-max'
    [U]::ShowWindow($script:A, 9) | Out-Null; Start-Sleep 2
    Display 100
    Start-Sleep 3
    Geo 'rt-back'; Snap 'rt-back'
    Surface 'rt-back-menu' { Click (XR 22) (Y 56) 1200 }
    # --- the same change with the window maximized: its size in pixels stays the same, so
    #     nothing but the DPI message says that the layout has to be done again ---
    [U]::ShowWindow($script:A, 3) | Out-Null; Start-Sleep 2
    Geo 'rt-max-before'; Snap 'rt-max-before'
    Display $Percent
    Start-Sleep 3
    Geo 'rt-max-changed'; Snap 'rt-max-changed'
    Surface 'rt-max-changed-menu' { Click (XR 22) (Y 56) 1200 }
    Surface 'rt-max-changed-tabmenu' { $tp = TabPoint $script:A 0; RightClick $tp[0] $tp[1] 1000 }
    Display 100
    Start-Sleep 3
    Geo 'rt-max-back'; Snap 'rt-max-back'
    [U]::ShowWindow($script:A, 9) | Out-Null; Start-Sleep 2
    Geo 'rt-restored'; Snap 'rt-restored'
    # --- restart at another scale: the bounds were saved at 100 % ---
    Focus $script:A | Out-Null; SafeKeys '%{F4}' 3000
    StopKrunk
    Display $Percent
    StartKrunk $prof $false
    Geo 'restart-other-scale'; Snap 'restart-other-scale'
    Focus $script:A | Out-Null; SafeKeys '%{F4}' 3000
    StopKrunk
    return
  }

  Display $Percent
  Fresh $prof $patch
  StartKrunk $prof
  Page "$W/dpi.html"
  Geo 'normal'; Snap 'main'
  Surface 'menu' { Click (XR 22) (Y 56) 1200 }
  Surface 'tabmenu' { $tp = TabPoint $script:A 0; RightClick $tp[0] $tp[1] 1000 }
  Surface 'pagemenu' { RightClick (PX 200) (PY 205) 1000 }
  Surface 'linkmenu' { RightClick (PX 80) (PY 162) 1000 }
XX
  Page "$W/dpi.html"
  Surface 'ai' { SafeKeys '^+ ' 2000; Geo 'ai' } { SafeKeys '^+ ' 800 }
  Surface 'alert' { Click (PX 500) (PY 380) 300; Keys 'a' 1500 } { Keys '{ENTER}' 600 }
  [U]::ShowWindow($script:A, 3) | Out-Null; Start-Sleep 2
  Geo 'max'; Snap 'max'
  Surface 'max-menu' { Click (XR 22) (Y 56) 1200 }
  [U]::ShowWindow($script:A, 9) | Out-Null; Start-Sleep 2
  Geo 'restored'

  if ($Set -eq 'all') {
    Surface 'imagemenu' { RightClick (PX 70) (PY 340) 1000 }
    Surface 'select' { Click (PX 300) (PY 266) 1000 }
    Surface 'tooltip' { MoveTo (PX 100) (PY 266); Start-Sleep -Milliseconds 1800 } { MoveTo (XR 150) (PY 420) }
    Surface 'status' { MoveTo (PX 80) (PY 162); Start-Sleep -Milliseconds 1200 } { MoveTo (XR 150) (PY 420) }
    Surface 'capmenu' { RightClick (XR 200) (Y 18) 1000 }
    Surface 'suggest' { SafeKeys '^l' 300; SafeKeys 'exa' 1500 } { Esc; Esc }
    Surface 'find' { SafeKeys '^f' 600; SafeKeys 'DPI' 800 }
    Surface 'siteinfo' { Click (X 133) (Y 56) 1200 }
    Surface 'shield' { Click (XR 90) (Y 56) 1200 }
    Surface 'bmfolder' { Click (X 40) (Y 90) 1200 }
    Surface 'bmmenu' { RightClick (X 40) (Y 90) 1200 }
    Surface 'perm' { Click (PX 500) (PY 380) 300; Keys 'g' 1800 }
    foreach ($step in 1..3) { Page "$W/nav.html?p=$step" 1500 "P$step" }   # not $k: that is $K
    Surface 'navmenu' { RightClick (X 22) (Y 56) 1200 }
    Surface 'auth' { SafeGo 'http://127.0.0.1:8110/' 3000 }
    Page "$W/dpi.html"
    Surface 'devtools' { SafeKeys '{F12}' 5000; Geo 'devtools' } { Focus $script:A | Out-Null; Keys '{F12}' 1500 }
    Page "$W/pip.html#one" 3500 'PIP'
    Surface 'pip' { SafeKeys '%p' 3000; KrunkWindows "$T pip" } { Focus $script:A | Out-Null; SafeKeys '%p' 1500 }
    foreach ($pg in @('downloads', 'extensions', 'history', 'sessions', 'about')) { Page "krunk://$pg" 2500; Snap "page-$pg" }
    SafeKeys '^t' 2000; Snap 'newtab'
    Surface 'tabhover' { $tp = TabPoint $script:A 0 2; MoveTo $tp[0] $tp[1]; Start-Sleep -Milliseconds 1800 } { MoveTo (XR 150) (PY 420) }
    SafeKeys '^w' 1000
    Page "$W/dpi.html"
    Click (PX 500) (PY 380) 300
    SafeKeys '{F11}' 2500; Geo 'full'; Snap 'full'; SafeKeys '{F11}' 2500
    Geo 'after-full'
    # Snap to the left half (Win+Left), Esc for the snap assistant.
    Focus $script:A | Out-Null
    [U]::keybd_event(0x5B, 0, 0, [UIntPtr]::Zero); [U]::keybd_event(0x25, 0, 0, [UIntPtr]::Zero)
    [U]::keybd_event(0x25, 0, 2, [UIntPtr]::Zero); [U]::keybd_event(0x5B, 0, 2, [UIntPtr]::Zero)
    Start-Sleep 2; Esc 1000
    Geo 'snapped'; Snap 'snapped'
    $s = $c.Scale
    [U]::ShowWindow($script:A, 9) | Out-Null
    [U]::MoveWindow($script:A, [int](8 * $s), [int](8 * $s), [int](960 * $s), [int](560 * $s), $true) | Out-Null; Start-Sleep 1
    Geo 'placed-again'
    # A second window and a private tab.
    Focus $script:A | Out-Null
    SafeKeys '^n' 3500
    $B = [U]::GetForegroundWindow()
    if ($B -ne $script:A -and [TW]::Cls($B) -eq 'KrunkWindow') { Geo 'win2' $B; Snap 'win2'; SafeKeys '%{F4}' 2000 } else { Note 'win2: no second window in front' }
    Focus $script:A | Out-Null
    SafeKeys '^+n' 3000; Snap 'private'; SafeKeys '^w' 1000
  }
  # Leave by Alt+F4 (bounds saved), start again at the same scale.
  Focus $script:A | Out-Null; SafeKeys '%{F4}' 3000
  StopKrunk
  StartKrunk $prof $false
  Geo 'restart'; Snap 'restart'
  Get-Content "$prof\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String 'WARN|ERROR|panic' | Select-Object -First 30 | ForEach-Object { Note "log: $($_.Line)" }
} catch {
  Note "ERROR $_"
} finally {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  try { & "$K\vm-display.ps1" -Restore | ForEach-Object { Note "display: $_" } } catch { Note "display: RESTORE FAILED $_" }
}
