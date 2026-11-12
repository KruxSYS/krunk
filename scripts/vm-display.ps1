# Reports or changes the test VM's display mode and scale, for the DPI passes (150 % and more
# need a larger mode than the VM's usual 1280x800, where Windows allows 125 % at most).
# Runs on the interactive desktop (scripts/vm.sh ps1 with a wrapper, or from another VM script):
#   vm-display.ps1                                   report only
#   vm-display.ps1 -Width 1920 -Height 1080 -Percent 150
#   vm-display.ps1 -Restore                          back to 1280x800 @ 100 %, verified
# The VM is shared: a change is refused while another project's scheduled task runs (-Force
# overrides), the original mode and scale are recorded once in display-original.txt, and every
# caller must end with -Restore (in a finally). The mode change is not written to the registry,
# so a restart of the VM also brings the usual mode back.
# Output goes to stdout and to C:\pipeline\krunk\shots\display.log.
param([int]$Width = 0, [int]$Height = 0, [int]$Percent = 0, [switch]$Restore, [switch]$Force)
$ErrorActionPreference = 'Stop'
$Home0 = 'C:\pipeline\krunk'
$Record = "$Home0\display-original.txt"
$LogFile = "$Home0\shots\display.log"
New-Item -ItemType Directory -Force "$Home0\shots" | Out-Null
function Say([string]$s) { Add-Content $LogFile "$(Get-Date -Format HH:mm:ss.fff) $s"; $s }
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Disp {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
    public int dmFields;
    public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels;
    public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int ChangeDisplaySettingsEx(string dev, ref DEVMODE dm, IntPtr hwnd, uint flags, IntPtr lp);
  static DEVMODE New() { var d = new DEVMODE(); d.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); return d; }
  public static string Current() { var d = New(); EnumDisplaySettings(null, -1, ref d); return d.dmPelsWidth + "x" + d.dmPelsHeight; }
  public static bool Has(int w, int h) {
    var d = New();
    for (int i = 0; EnumDisplaySettings(null, i, ref d); i++) if (d.dmPelsWidth == w && d.dmPelsHeight == h) return true;
    return false;
  }
  // 0 = DISP_CHANGE_SUCCESSFUL. Flags 0: dynamic, not saved.
  public static int Set(int w, int h) {
    var d = New(); EnumDisplaySettings(null, -1, ref d);
    d.dmPelsWidth = w; d.dmPelsHeight = h; d.dmFields = 0x80000 | 0x100000;
    return ChangeDisplaySettingsEx(null, ref d, IntPtr.Zero, 0, IntPtr.Zero);
  }
}
'@
function Scale([int]$p = 0) {
  $a = @('-NoProfile', '-File', "$Home0\vm-dpi.ps1"); if ($p -gt 0) { $a += @('-Percent', "$p") }
  (& powershell @a | Out-String).Trim() -replace '\s+', ' '
}
function Report { "mode=$([Disp]::Current()) scale: $(Scale)" }
function Others {
  @(Get-ScheduledTask | Where-Object { $_.TaskPath -eq '\' -and $_.State -eq 'Running' -and $_.TaskName -ne 'KrunkTask' } | ForEach-Object { $_.TaskName })
}
function Apply([int]$w, [int]$h, [int]$p) {
  if ([Disp]::Current() -ne "${w}x$h") {
    if (-not [Disp]::Has($w, $h)) { throw "the display has no ${w}x$h mode" }
    $r = [Disp]::Set($w, $h); Say "set mode ${w}x${h}: $r" | Out-Null
    if ($r -ne 0) { throw "ChangeDisplaySettingsEx returned $r" }
    Start-Sleep -Seconds 3
  }
  Say "scale: $(Scale $p)" | Out-Null
  Start-Sleep -Seconds 3
}
if ($Restore) {
  Apply 1280 800 100
  $now = Report
  Say "restored: $now"
  if ($now -notmatch '^mode=1280x800 scale: current=100%') { throw "restore failed: $now" }
  Remove-Item $Record -ErrorAction SilentlyContinue
  return
}
if ($Width -eq 0 -and $Percent -eq 0) { Say (Report); return }
$busy = Others
if ($busy.Count -gt 0 -and -not $Force) { Say "refused: other tasks are running: $($busy -join ', ')"; throw 'the VM is in use' }
if (-not (Test-Path $Record)) { Set-Content $Record (Report) }
if ($Width -eq 0) { $cur = [Disp]::Current() -split 'x'; $Width = [int]$cur[0]; $Height = [int]$cur[1] }
if ($Percent -eq 0) { $Percent = 100 }
Apply $Width $Height $Percent
$now = Report
Say "now: $now"
if ($now -notmatch "^mode=${Width}x$Height scale: current=$Percent%") { throw "not applied: $now" }
