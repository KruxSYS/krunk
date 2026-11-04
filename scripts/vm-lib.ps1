# Helpers for VM test scripts (dot-source: . C:\pipeline\krunk\lib.ps1). They run in the
# interactive desktop session through the KrunkTask scheduled task (scripts/vm.sh ps1).
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class U {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
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
function Move([int]$x, [int]$y) { [U]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 80 }
function Click([int]$x, [int]$y, [int]$wait = 300) {
  Move $x $y; [U]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [U]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function DoubleClick([int]$x, [int]$y) {
  Move $x $y
  for ($i = 0; $i -lt 2; $i++) { [U]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [U]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 70 }
  Start-Sleep -Milliseconds 400
}
function RightClick([int]$x, [int]$y, [int]$wait = 500) {
  Move $x $y; [U]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [U]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function Drag([int]$x1, [int]$y1, [int]$x2, [int]$y2, [int]$steps = 20) {
  Move $x1 $y1; [U]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 150
  for ($i = 1; $i -le $steps; $i++) { Move ($x1 + ($x2 - $x1) * $i / $steps) ($y1 + ($y2 - $y1) * $i / $steps) }
  Start-Sleep -Milliseconds 150; [U]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 300
}
function MainWindow([string]$class = 'KrunkWindow') { [U]::FindWindow($class, $null) }
function WindowRect([IntPtr]$h) { $r = New-Object U+RECT; [U]::GetWindowRect($h, [ref]$r) | Out-Null; $r }
function Log([string]$line) { Add-Content "$Shots\run.log" "$(Get-Date -Format HH:mm:ss.fff) $line" }
function Go([string]$url, [int]$wait = 2500) { Keys '^l'; Keys $url 100; Keys '{ENTER}' $wait }
