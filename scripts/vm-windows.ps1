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
