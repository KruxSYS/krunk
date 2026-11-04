# Lists or changes the primary display resolution on the VM (for the 150 % DPI test, which
# Windows only offers from about 1600x900 up):  vm-res.ps1 [-Width 1920 -Height 1080]
param([int]$Width = 0, [int]$Height = 0)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Res {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
    public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool EnumDisplaySettings(string d, int i, ref DEVMODE m);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int ChangeDisplaySettings(ref DEVMODE m, int f);
  public static string Modes() {
    var m = new DEVMODE(); m.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); var seen = new System.Collections.Generic.HashSet<string>();
    for (int i = 0; EnumDisplaySettings(null, i, ref m); i++) seen.Add(m.dmPelsWidth + "x" + m.dmPelsHeight);
    return string.Join(" ", seen);
  }
  public static string Current() { var m = new DEVMODE(); m.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); EnumDisplaySettings(null, -1, ref m); return m.dmPelsWidth + "x" + m.dmPelsHeight; }
  public static int Set(int w, int h) { var m = new DEVMODE(); m.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); EnumDisplaySettings(null, -1, ref m); m.dmPelsWidth = w; m.dmPelsHeight = h; m.dmFields = 0x180000; return ChangeDisplaySettings(ref m, 0); }
}
'@
if ($Width -gt 0) { "set ${Width}x${Height}: $([Res]::Set($Width, $Height))" }
"current $([Res]::Current()); modes: $([Res]::Modes())"
