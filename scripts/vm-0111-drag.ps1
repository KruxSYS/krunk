param([string]$Label = 'after', [int]$Full = 1)
# Krunk 0.11.1 VM flow: tab dragging (smoothness, the ghost tab) and the menus after a merge.
#   1  A tear-off recorded with tools\framecap while a C# driver moves the mouse at a constant
#      speed (absolute moves every 8 ms, 1 ms timer resolution): the torn window's page has a
#      one-colour square (marker.html), analyze.py track follows it on the build host. The
#      recording's PATH.json has its start QPC and $P-moves.csv each move's, for the lag. With
#      -Full 0 only this part runs (the 0.11.0 baseline uses the same steps).
#   2  Open ⋮ in the torn window, then drag its only tab over the first window's strip and hold
#      (the ghost shot), off the strip and back, and drop: the windows merge.
#   3  ⋮, the tab menu, the Back history menu and the page's menu still open (0.11.0: a menu
#      shown in a window that closed died with it, and every later menu with it).
#   4  Tear a tab off again and press Esc while holding: it goes back.
# Needs serve.ps1 on 8094 (started here). Screenshots "drag-<label>-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "drag-$Label"
$K = 'C:\pipeline\krunk'
$profile = "$K\p0111d"
Add-Type @"
using System; using System.Diagnostics; using System.Threading; using System.Runtime.InteropServices;
public static class Drv {
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint p);
  [DllImport("winmm.dll")] static extern uint timeEndPeriod(uint p);
  [StructLayout(LayoutKind.Sequential)] struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct IN { public uint type; public MI mi; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  static void Send(int dx, int dy, uint flags) { var i = new IN[1]; i[0].type = 0; i[0].mi.dx = dx; i[0].mi.dy = dy; i[0].mi.flags = flags; SendInput(1, i, Marshal.SizeOf(typeof(IN))); }
  public static void To(int x, int y) {
    int l = GetSystemMetrics(76), t = GetSystemMetrics(77), w = GetSystemMetrics(78), h = GetSystemMetrics(79);
    Send((int)((x - l) * 65535.0 / (w - 1)), (int)((y - t) * 65535.0 / (h - 1)), 0x8001 | 0x4000);
  }
  // Every move: its QPC time and screen point (framecap's PATH.json has the recording's start QPC).
  public static System.Collections.Generic.List<string> Moves = new System.Collections.Generic.List<string>();
  public static void Down() { Send(0, 0, 0x0002); }
  public static void Up() { Send(0, 0, 0x0004); }
  // n moves of (dx, dy) from (x, y), one every ms milliseconds.
  public static string Path(int x, int y, int dx, int dy, int n, int ms) {
    timeBeginPeriod(1);
    var sw = Stopwatch.StartNew(); double next = 0; int k = 0;
    while (k < n) {
      if (sw.Elapsed.TotalMilliseconds >= next) { k++; To(x + dx * k, y + dy * k); Moves.Add(Stopwatch.GetTimestamp() + "," + (x + dx * k) + "," + (y + dy * k)); next += ms; }
      Thread.Sleep(0);
    }
    timeEndPeriod(1);
    return n + " moves in " + Math.Round(sw.Elapsed.TotalMilliseconds) + " ms";
  }
}
"@
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function Popups { $pids = KrunkPids; @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' }) }
function Esc { [U]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [U]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero) }

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 900, 560, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
Log "[$P] Krunk $((Get-Item "$K\app\Krunk.dll").VersionInfo.FileVersion)"
SafeGo 'krunk://about' 1500
SafeKeys '^t' 900
SafeGo 'http://localhost:8094/marker.html' 1500
Start-Sleep 4                                    # 0.11.1: the spare frame is prepared 3 s after the last shell loaded
Log "[$P] Krunk processes before the tear-off: $(@(KrunkPids).Count)"

# ---- 1: the measured tear-off.
$t = TabPoint $A 1 2
$cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', '0,0,1280,800', '--step', '4', '--seconds', '5', '--out', "$Shots\$P-tear" -WindowStyle Hidden -PassThru
Start-Sleep -Milliseconds 700
[Drv]::To($t[0], $t[1]); Start-Sleep -Milliseconds 150
[Drv]::Down(); Start-Sleep -Milliseconds 200
$r1 = [Drv]::Path($t[0], $t[1], 0, 8, 12, 8)            # 96 px down: out of the strip
Shot "$P-1-torn-early"
$r2 = [Drv]::Path($t[0], $t[1] + 96, 3, 1, 200, 8)      # 1.6 s at 3,1 px per 8 ms
Start-Sleep -Milliseconds 300
[Drv]::Up()
$cap.WaitForExit(15000) | Out-Null
Log "[$P] tear-off moves: $r1; then $r2; capture: $(Get-Content "$Shots\$P-tear.json")"
(@("qpc,x,y,qpf=$([System.Diagnostics.Stopwatch]::Frequency)") + [Drv]::Moves) | Set-Content "$Shots\$P-moves.csv"
Start-Sleep -Milliseconds 800
Shot "$P-1-dropped"
KrunkWindows "$P-1"
if ($Full -eq 0) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Log "[$P] done"
  return
}

# ---- 2: a menu in the torn window, then its only tab over the first window's strip (ghost).
$B = FrameByTitle 'Marker*'
Log "[$P] torn window: '$([TW]::Txt($B))'"
[U]::MoveWindow($B, 360, 200, 900, 560, $true) | Out-Null; Start-Sleep -Milliseconds 500   # its ⋮ on screen
Focus $B | Out-Null
$cb = ClientRect $B
Click ($cb.Right - 20) ($cb.Top + [int](56 * $cb.Scale)) 900
Log "[$P] ⋮ in the torn window: popups visible $(@(Popups).Count)"
Esc; Start-Sleep -Milliseconds 400
$tb = TabPoint $B 0 1
$ta = TabPoint $A 0 1
[Drv]::To($tb[0], $tb[1]); Start-Sleep -Milliseconds 150
[Drv]::Down(); Start-Sleep -Milliseconds 200
$null = [Drv]::Path($tb[0], $tb[1], 0, 6, 10, 8)
$x0 = $tb[0]; $y0 = $tb[1] + 60
$n = 40
$null = [Drv]::Path($x0, $y0, [int](($ta[0] + 150 - $x0) / $n), [int](($ta[1] - $y0) / $n), $n, 8)
$x1 = $x0 + [int](($ta[0] + 150 - $x0) / $n) * $n; $y1 = $y0 + [int](($ta[1] - $y0) / $n) * $n
Start-Sleep -Milliseconds 600
Shot "$P-2-ghost-over-strip"
KrunkWindows "$P-2-hover"
$null = [Drv]::Path($x1, $y1, 0, 10, 20, 8)
Start-Sleep -Milliseconds 500
Shot "$P-2-off-the-strip"
$null = [Drv]::Path($x1, $y1 + 200, 0, -10, 20, 8)
Start-Sleep -Milliseconds 400
[Drv]::Up(); Start-Sleep -Milliseconds 1200
Shot "$P-2-merged"
KrunkWindows "$P-2-merged"

# ---- 3: menus after the merge.
Focus $A | Out-Null
$c = ClientRect $A
Click ($c.Right - 20) ($c.Top + [int](56 * $c.Scale)) 900
Log "[$P] after the merge: ⋮ popups visible $(@(Popups).Count)"
Shot "$P-3-main-menu"
Esc; Start-Sleep -Milliseconds 400
$t0 = TabPoint $A 0 2
RightClick $t0[0] $t0[1] 900
Log "[$P] after the merge: tab menu popups visible $(@(Popups).Count)"
Shot "$P-3-tab-menu"
Esc; Start-Sleep -Milliseconds 400
RightClick ($c.Left + [int](20 * $c.Scale)) ($c.Top + [int](56 * $c.Scale)) 900
Log "[$P] after the merge: Back history menu popups visible $(@(Popups).Count)"
Shot "$P-3-history-menu"
Esc; Start-Sleep -Milliseconds 400
RightClick ($c.Left + 400) ($c.Top + 300) 900
Shot "$P-3-page-menu"
Esc; Start-Sleep -Milliseconds 400

# ---- 4: Esc while a torn-off tab is held puts it back.
$t = TabPoint $A 1 2
[Drv]::To($t[0], $t[1]); Start-Sleep -Milliseconds 150
[Drv]::Down(); Start-Sleep -Milliseconds 200
$null = [Drv]::Path($t[0], $t[1], 2, 8, 15, 8)
Start-Sleep -Milliseconds 300
Shot "$P-4-torn"
Log "[$P] torn again: frames $(@(Frames).Count)"
Esc; Start-Sleep -Milliseconds 400
[Drv]::Up(); Start-Sleep -Milliseconds 900
Log "[$P] after Esc: frames $(@(Frames).Count), title '$([TW]::Txt($A))'"
Shot "$P-4-cancelled"
Log "[$P] Krunk processes: $(@(KrunkPids).Count)"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'tab \d+:|window \d+|spare|WARN|ERROR|popup' | ForEach-Object { Log "[log] $_" }
Log "[$P] done"
