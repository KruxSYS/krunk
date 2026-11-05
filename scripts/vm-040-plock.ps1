# Krunk 0.4.0 milestone 3: pointer-lock movement smoothness. For each scenario the recorder
# page (vm-www/8090/plock-measure.html) locks the pointer; a driver then sends relative mouse
# moves at a steady rate (5 px every ~1 ms for 2 s), optionally while holding the left and right
# buttons and repeating the A key (the director's case: walk with both buttons, strafe with A,
# turn with the mouse). The page reports per-frame movement statistics, which are copied from
# the test server's access log into run.log. Fresh profile; needs serve.ps1 on :8090.
param([string]$Only = '')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'plock'
Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
public static class Drive {
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint p);
  [DllImport("winmm.dll")] static extern uint timeEndPeriod(uint p);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct IN { public uint type; public MI mi; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  static void Mouse(uint flags, int dx, int dy) {
    var a = new IN[1]; a[0].type = 0; a[0].mi.flags = flags; a[0].mi.dx = dx; a[0].mi.dy = dy;
    SendInput(1, a, Marshal.SizeOf(typeof(IN)));
  }
  // Moves (dx per tick) for ms; buttons held; key A repeated every 33 ms. Returns moves sent.
  public static int Run(int ms, int dx, double tickMs, bool buttons, bool key) {
    timeBeginPeriod(1);
    int sent = 0;
    try {
      if (buttons) { Mouse(2, 0, 0); Thread.Sleep(30); Mouse(8, 0, 0); Thread.Sleep(30); }
      var sw = Stopwatch.StartNew();
      double nextMove = 0, nextKey = 0;
      while (sw.Elapsed.TotalMilliseconds < ms) {
        double t = sw.Elapsed.TotalMilliseconds;
        if (t >= nextMove) { Mouse(1, dx, 0); sent++; nextMove += tickMs; }
        if (key && t >= nextKey) { keybd_event(0x41, 0x1E, 0, UIntPtr.Zero); nextKey += 33; }
        Thread.Sleep(0);
      }
      if (key) keybd_event(0x41, 0x1E, 2, UIntPtr.Zero);
      if (buttons) { Mouse(16, 0, 0); Thread.Sleep(30); Mouse(4, 0, 0); }
    } finally { timeEndPeriod(1); }
    return sent;
  }
}
'@
$scenarios = @(
  @{ label = 'plain'; unadj = $false; buttons = $false; key = $false; listen = 'mousemove' },
  @{ label = 'held'; unadj = $false; buttons = $true; key = $true; listen = 'mousemove' },
  @{ label = 'plain-raw'; unadj = $true; buttons = $false; key = $false; listen = 'mousemove' },
  @{ label = 'held-raw'; unadj = $true; buttons = $true; key = $true; listen = 'mousemove' },
  @{ label = 'held-all-events'; unadj = $false; buttons = $true; key = $true; listen = 'mousemove,pointermove,pointerrawupdate' }
)
if ($Only) { $scenarios = $scenarios | Where-Object { $_.label -like $Only } }

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8090' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8090/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p040p'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
$logStart = (Get-Content C:\pipeline\krunk\www\access.log -ErrorAction SilentlyContinue | Measure-Object -Line).Lines

foreach ($s in $scenarios) {
  $url = "http://localhost:8090/plock-measure.html?label=$($s.label)&ms=3000&listen=$($s.listen)" + $(if ($s.unadj) { '&unadjusted=1' } else { '' })
  SafeGo $url 2500
  # Lock: click the stage (a user gesture).
  Click ([int](($c.Left + $c.Right) / 2)) ([int]($c.Top + 300 * $c.Scale)) 500
  $sent = [Drive]::Run(2000, 5, 1.0, $s.buttons, $s.key)
  Log "[$($s.label)] driver sent $sent moves"
  Start-Sleep 2
  Shot "$P-$($s.label)"
  Focus $A | Out-Null
}
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 2
Get-Content C:\pipeline\krunk\www\access.log | Select-Object -Skip $logStart | Where-Object { $_ -like '*plock-result*' } | ForEach-Object {
  $q = ($_ -split 'plock-result\?')[1] -split ' ' | Select-Object -First 1
  Log "[result] $([Uri]::UnescapeDataString($q))"
}
Get-Content "$profile\Krunk\krunk.log" | Select-String 'pointer|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
