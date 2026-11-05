# Lists the top-level windows of Krunk's processes (class, title, visibility, rect) into the run
# log: shows how many browser windows exist and that DevTools has none of its own.
# Dot-source after lib.ps1, then call: KrunkWindows 'label'
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class TW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public static List<IntPtr> All() { var l = new List<IntPtr>(); EnumWindows((h, p) => { l.Add(h); return true; }, IntPtr.Zero); return l; }
  public static uint Pid(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
  public static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string Txt(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
}
'@
function KrunkWindows([string]$label) {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $n = 0
  foreach ($h in [TW]::All()) {
    if ($pids -notcontains [TW]::Pid($h)) { continue }
    $vis = [TW]::IsWindowVisible($h)
    if (-not $vis) { continue }
    $r = WindowRect $h
    Log "[$label] top-level window class=$([TW]::Cls($h)) title='$([TW]::Txt($h))' rect=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
    $n++
  }
  Log "[$label] visible top-level Krunk windows: $n"
}

# The visible Krunk frame whose title matches (newest match first in z-order).
function FrameByTitle([string]$like) {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  foreach ($h in [TW]::All()) {
    if ($pids -notcontains [TW]::Pid($h) -or -not [TW]::IsWindowVisible($h)) { continue }
    if ([TW]::Cls($h) -eq 'KrunkWindow' -and [TW]::Txt($h) -like $like) { return $h }
  }
  [IntPtr]::Zero
}
# Every visible Krunk frame.
function Frames {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkWindow' })
}
# Screen point 40 DIP into tab $i of $n in frame $h (tabs are 256 DIP wide from x = 8 DIP and
# shrink evenly when the strip is narrower; clear of the close button).
function TabPoint([IntPtr]$h, [int]$i, [int]$n = 1) {
  $c = ClientRect $h
  $avail = ($c.Right - $c.Left) / $c.Scale - 222
  $w = [Math]::Min(256, $avail / [Math]::Max(1, $n))
  @([int]($c.Left + (8 + ($w + 2) * $i + 40) * $c.Scale), [int]($c.Top + 18 * $c.Scale))
}

# Brings a window to the front (an Alt press lets SetForegroundWindow succeed from the task).
function Focus([IntPtr]$h) {
  [U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [U]::SetForegroundWindow($h) | Out-Null
  [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 400
  [U]::GetForegroundWindow() -eq $h
}
# Types only while a Krunk frame is in front: stray keys on the desktop can start other apps.
function SafeKeys([string]$k, [int]$wait = 300) {
  $fg = [U]::GetForegroundWindow()
  if ([TW]::Cls($fg) -ne 'KrunkWindow') { Log "SafeKeys: foreground is '$([TW]::Cls($fg))', not Krunk; not typing '$k'"; throw "focus lost" }
  Keys $k $wait
}
function SafeGo([string]$url, [int]$wait = 2500) { SafeKeys '^l'; SafeKeys $url 100; SafeKeys '{ENTER}' $wait }
