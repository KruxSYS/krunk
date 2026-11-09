# Helpers for VM test scripts (dot-source: . C:\pipeline\krunk\lib.ps1). They run in the
# interactive desktop session through the KrunkTask scheduled task (scripts/vm.sh ps1).
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class U {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class SI {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public MOUSEINPUT mi; }
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] i, int size);
  public static uint Mouse(uint flags, int dx, int dy) { var a = new INPUT[1]; a[0].type = 0; a[0].mi.dwFlags = flags; a[0].mi.dx = dx; a[0].mi.dy = dy; return SendInput(1, a, Marshal.SizeOf(typeof(INPUT))); }
  public static uint Wheel(int delta) { var a = new INPUT[1]; a[0].type = 0; a[0].mi.dwFlags = 0x0800; a[0].mi.mouseData = (uint)delta; return SendInput(1, a, Marshal.SizeOf(typeof(INPUT))); }
}
'@
# Physical pixels everywhere (screenshots, cursor, window rects), also after a live DPI
# change: per-monitor-v2 awareness (-4).
[U]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$Shots = 'C:\pipeline\krunk\shots'
New-Item -ItemType Directory -Force $Shots | Out-Null
function Shot([string]$name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
  $bmp.Save("$Shots\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function Keys([string]$k, [int]$wait = 300) { [System.Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait }
# Real mouse-move input (absolute, normalized to 0..65535): the window
# move/size loop ignores cursor jumps made with SetCursorPos.
# (Not "Move": that is a built-in alias of Move-Item and would win.)
function MoveTo([int]$x, [int]$y) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $nx = [int](($x - $b.Left) * 65535 / ($b.Width - 1)); $ny = [int](($y - $b.Top) * 65535 / ($b.Height - 1))
  [U]::SetCursorPos($x, $y) | Out-Null; [U]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40
}
function Click([int]$x, [int]$y, [int]$wait = 300) {
  [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 150
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 60; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds $wait
}
function DoubleClick([int]$x, [int]$y) {
  [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 300
  for ($i = 0; $i -lt 2; $i++) { [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 30; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 60 }
  Start-Sleep -Milliseconds 500
}
# Relative mouse movement (what games read): $steps reports of ($dx, $dy).
function RelMove([int]$dx, [int]$dy, [int]$steps = 1, [int]$sleepMs = 8) {
  for ($i = 0; $i -lt $steps; $i++) { [SI]::Mouse(1, $dx, $dy) | Out-Null; Start-Sleep -Milliseconds $sleepMs }
}
# Mouse wheel at a point: notches > 0 scroll down.
function Wheel([int]$x, [int]$y, [int]$notches) {
  [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 100
  for ($i = 0; $i -lt [Math]::Abs($notches); $i++) { [SI]::Wheel($(if ($notches -gt 0) { -120 } else { 120 })) | Out-Null; Start-Sleep -Milliseconds 60 }
  Start-Sleep -Milliseconds 400
}
function RightClick([int]$x, [int]$y, [int]$wait = 500) {
  [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 150
  [SI]::Mouse(8, 0, 0) | Out-Null; Start-Sleep -Milliseconds 60; [SI]::Mouse(16, 0, 0) | Out-Null; Start-Sleep -Milliseconds $wait
}
# Window moves and resizes need SendInput moves and a hold after the press
# (the modal move/size loop starts late); page drags work the same way.
function AbsMove([int]$x, [int]$y) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  [SI]::Mouse(0x8001, [int](($x - $b.Left) * 65535 / ($b.Width - 1)), [int](($y - $b.Top) * 65535 / ($b.Height - 1))) | Out-Null
}
function Drag([int]$x1, [int]$y1, [int]$x2, [int]$y2, [int]$steps = 20) {
  [U]::SetCursorPos($x1, $y1) | Out-Null; AbsMove $x1 $y1; Start-Sleep -Milliseconds 150
  [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 500
  for ($i = 1; $i -le $steps; $i++) { AbsMove ($x1 + ($x2 - $x1) * $i / $steps) ($y1 + ($y2 - $y1) * $i / $steps); Start-Sleep -Milliseconds 30 }
  Start-Sleep -Milliseconds 300; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 400
}
# The first visible window of the class (0.11.1: Krunk keeps a hidden spare frame of its own class).
function MainWindow([string]$class = 'KrunkWindow') {
  $h = [IntPtr]::Zero
  do { $h = [U]::FindWindowEx([IntPtr]::Zero, $h, $class, [NullString]::Value) } while ($h -ne [IntPtr]::Zero -and -not [U]::IsWindowVisible($h))
  $h
}
function WindowRect([IntPtr]$h) { $r = New-Object U+RECT; [U]::GetWindowRect($h, [ref]$r) | Out-Null; $r }
# Client area of a window in screen coordinates, and its DPI scale.
function ClientRect([IntPtr]$h) {
  $c = New-Object U+RECT; [U]::GetClientRect($h, [ref]$c) | Out-Null
  $p = New-Object U+POINT; [U]::ClientToScreen($h, [ref]$p) | Out-Null
  [pscustomobject]@{ Left = $p.X; Top = $p.Y; Right = $p.X + $c.Right; Bottom = $p.Y + $c.Bottom; Scale = [U]::GetDpiForWindow($h) / 96.0 }
}
function Log([string]$line) { Add-Content "$Shots\run.log" "$(Get-Date -Format HH:mm:ss.fff) $line" }
function Go([string]$url, [int]$wait = 2500) { Keys '^l'; Keys $url 100; Keys '{ENTER}' $wait }
