param([string]$Label = 'after', [int]$Chrome = 0, [int]$Overlays = 1)
# Krunk 0.7.2 M6 (R7): turning the camera while a movement key is held. jitter.html records the
# mousemove events of a 3 s turn: with the right button held (no lock) and in pointer lock, each
# without and with W held. A C# helper sends a relative move of 4 px every 8 ms (125 Hz) with
# SendInput, and W keydowns like Windows' auto-repeat (down, then every 33 ms from 0.5 s on); 1 ms
# timer resolution, a stopwatch for the timing. The page sends its summary to /log?jitter=...;
# the lines are copied into run.log. -Chrome 1 runs the same four rows in Chrome 157 (the
# reference). -Overlays 1 adds the 0.5.0 overlay rows: the lock hint, Esc ends the lock, F11.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "jitter-$Label"
$K = 'C:\pipeline\krunk'
Add-Type @"
using System; using System.Diagnostics; using System.Threading; using System.Runtime.InteropServices;
public static class Drive {
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint p);
  [DllImport("winmm.dll")] static extern uint timeEndPeriod(uint p);
  [StructLayout(LayoutKind.Sequential)] struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] struct UN { [FieldOffset(0)] public MI mi; [FieldOffset(0)] public KI ki; }
  [StructLayout(LayoutKind.Sequential)] struct IN { public uint type; public UN u; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  static void Mouse(int dx, int dy, uint flags) { var i = new IN[1]; i[0].type = 0; i[0].u.mi.dx = dx; i[0].u.mi.dy = dy; i[0].u.mi.flags = flags; SendInput(1, i, Marshal.SizeOf(typeof(IN))); }
  static void Key(bool up) { var i = new IN[1]; i[0].type = 1; i[0].u.ki.vk = 0x57; i[0].u.ki.scan = 0x11; i[0].u.ki.flags = up ? 2u : 0u; SendInput(1, i, Marshal.SizeOf(typeof(IN))); }
  public static string Run(int ms, bool holdW, bool right) {
    timeBeginPeriod(1);
    if (right) { Mouse(0, 0, 0x0008); Thread.Sleep(150); }
    var sw = Stopwatch.StartNew();
    double next = 0, nextKey = holdW ? 0 : double.MaxValue; int moves = 0, keys = 0; bool down = false;
    while (sw.Elapsed.TotalMilliseconds < ms) {
      double t = sw.Elapsed.TotalMilliseconds;
      if (t >= nextKey) { Key(false); keys++; nextKey = down ? nextKey + 33 : 500; down = true; }
      if (t >= next) { Mouse(4, 0, 0x0001); moves++; next += 8; }
      Thread.Sleep(0);
    }
    if (down) Key(true);
    if (right) { Thread.Sleep(50); Mouse(0, 0, 0x0010); }
    timeEndPeriod(1);
    return moves + " moves, " + keys + " W keydowns in " + Math.Round(sw.Elapsed.TotalMilliseconds) + " ms";
  }
}
"@
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$logStart = (Get-Content "$K\www\access.log" -ErrorAction SilentlyContinue | Measure-Object -Line).Lines
$base = 'http://localhost:8094/jitter.html'
$rows = @(@('right', $false), @('right', $true), @('lock', $false), @('lock', $true))

# ---- Krunk
$profile = "$K\p072j"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
Log "[$P] Krunk $((Get-Item "$K\app\Krunk.dll").VersionInfo.FileVersion)"
foreach ($r in $rows) {
  $mode = $r[0]; $w = $r[1]; $name = "krunk-$Label-$mode-$(if ($w) { 'W' } else { 'noW' })"
  SafeGo "$base`?mode=$mode&label=$name&ms=3000" 2000
  MoveTo 640 450; Start-Sleep -Milliseconds 300
  if ($mode -eq 'lock') { Click 640 450 500; $res = [Drive]::Run(3300, $w, $false) } else { $res = [Drive]::Run(3000, $w, $true) }
  Start-Sleep -Milliseconds 800
  Log "[$P] $name sent: $res"
}
Shot "$P-1-krunk-last"
if ($Overlays -eq 1) {
  # (0.5.0 overlays) The lock shows the hint, Esc ends the lock, F11 enters and leaves full screen.
  SafeGo "$base`?mode=lock&label=overlay&ms=60000" 2000
  Click 640 450 700
  Shot "$P-2-locked-hint"
  $lockLog = Select-String -Path "$profile\Krunk\krunk.log" -Pattern 'pointer locked' -Quiet
  Keys '{ESC}' 700
  $unlockLog = Select-String -Path "$profile\Krunk\krunk.log" -Pattern 'pointer unlocked' -Quiet
  Log "[$P] overlay: locked logged $lockLog, Esc unlocked logged $unlockLog"
  SafeKeys '{F11}' 1500
  $fr = WindowRect $A; Log "[$P] F11: window $($fr.Left),$($fr.Top)-$($fr.Right),$($fr.Bottom)"
  Shot "$P-3-fullscreen"
  SafeKeys '{F11}' 1500
  $fr = WindowRect $A; Log "[$P] F11 again: window $($fr.Left),$($fr.Top)-$($fr.Right),$($fr.Bottom)"
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue

# ---- Chrome 157 (the reference)
if ($Chrome -eq 1) {
  $cr = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
  $crp = "$K\cr072"
  Remove-Item -Recurse -Force $crp -ErrorAction SilentlyContinue
  $args0 = @("--user-data-dir=$crp", '--no-first-run', '--no-default-browser-check', '--window-position=0,0', '--window-size=1280,760')
  foreach ($r in $rows) {
    $mode = $r[0]; $w = $r[1]; $name = "chrome-$mode-$(if ($w) { 'W' } else { 'noW' })"
    Start-Process -FilePath $cr -ArgumentList ($args0 + "$base`?mode=$mode&label=$name&ms=3000")
    Start-Sleep 4
    $h = (Get-Process chrome -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowHandle
    if ($h) { [U]::SetForegroundWindow($h) | Out-Null }
    MoveTo 640 450; Start-Sleep -Milliseconds 300
    if ($mode -eq 'lock') { Click 640 450 600; $res = [Drive]::Run(3300, $w, $false) } else { $res = [Drive]::Run(3000, $w, $true) }
    Start-Sleep -Milliseconds 800
    Log "[$P] $name sent: $res"
  }
  Shot "$P-4-chrome-last"
  Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -Force
}
Start-Sleep 1
Get-Content "$K\www\access.log" | Select-Object -Skip $logStart | Select-String 'jitter=' | ForEach-Object {
  $m = [regex]::Match($_.Line, 'jitter=([^ ]+)'); Log "[result] $([uri]::UnescapeDataString($m.Groups[1].Value))"
}
Log "[$P] done"
