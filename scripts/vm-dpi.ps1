# Changes the display scale of the primary monitor live (no sign-out), for DPI tests on the VM:
#   powershell -File vm-dpi.ps1 [-Percent 150]     (without -Percent: only report)
# Uses DisplayConfigSetDeviceInfo with the undocumented DISPLAYCONFIG_DEVICE_INFO_SET_DPI_SCALE
# (-4) / GET (-3) requests, which take the scale as a step relative to the recommended one.
param([int]$Percent = 0)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Dpi {
  [StructLayout(LayoutKind.Sequential)] public struct LUID { public uint Low; public int High; }
  [StructLayout(LayoutKind.Sequential)] public struct HEADER { public int type; public uint size; public LUID adapterId; public uint id; }
  [StructLayout(LayoutKind.Sequential)] public struct GET { public HEADER header; public int minRel; public int curRel; public int maxRel; }
  [StructLayout(LayoutKind.Sequential)] public struct SET { public HEADER header; public int scaleRel; }
  [StructLayout(LayoutKind.Sequential)] public struct PATH_SOURCE { public LUID adapterId; public uint id; public uint modeInfoIdx; public uint statusFlags; }
  [StructLayout(LayoutKind.Sequential)] public struct RATIONAL { public uint n; public uint d; }
  [StructLayout(LayoutKind.Sequential)] public struct PATH_TARGET { public LUID adapterId; public uint id; public uint modeInfoIdx; public int outputTechnology; public int rotation; public int scaling; public RATIONAL refreshRate; public int scanLineOrdering; public int targetAvailable; public uint statusFlags; }
  [StructLayout(LayoutKind.Sequential)] public struct PATH { public PATH_SOURCE source; public PATH_TARGET target; public uint flags; }
  [StructLayout(LayoutKind.Sequential, Size = 64)] public struct MODE { public int infoType; public uint id; public LUID adapterId; }
  [DllImport("user32.dll")] public static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
  [DllImport("user32.dll")] public static extern int QueryDisplayConfig(uint flags, ref uint np, [Out] PATH[] p, ref uint nm, [Out] MODE[] m, IntPtr topology);
  [DllImport("user32.dll")] public static extern int DisplayConfigGetDeviceInfo(ref GET g);
  [DllImport("user32.dll")] public static extern int DisplayConfigSetDeviceInfo(ref SET s);
  public static readonly int[] Steps = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };
  static PATH First() {
    uint np, nm; GetDisplayConfigBufferSizes(2, out np, out nm);
    var p = new PATH[np]; var m = new MODE[nm];
    int r = QueryDisplayConfig(2, ref np, p, ref nm, m, IntPtr.Zero);
    if (r != 0 || np == 0) throw new Exception("QueryDisplayConfig failed: " + r);
    return p[0];
  }
  public static string Report() {
    var p = First(); var g = new GET(); g.header.type = -3; g.header.size = (uint)Marshal.SizeOf(typeof(GET));
    g.header.adapterId = p.source.adapterId; g.header.id = p.source.id;
    int r = DisplayConfigGetDeviceInfo(ref g); if (r != 0) return "get failed " + r;
    int rec = Math.Abs(g.minRel);
    return "current=" + Steps[rec + g.curRel] + "% recommended=" + Steps[rec] + "% max=" + Steps[rec + g.maxRel] + "%";
  }
  public static string Set(int percent) {
    var p = First(); var g = new GET(); g.header.type = -3; g.header.size = (uint)Marshal.SizeOf(typeof(GET));
    g.header.adapterId = p.source.adapterId; g.header.id = p.source.id;
    DisplayConfigGetDeviceInfo(ref g);
    int want = Array.IndexOf(Steps, percent); if (want < 0) return "unsupported percent";
    int rec = Math.Abs(g.minRel); int rel = want - rec;
    if (rel < g.minRel || rel > g.maxRel) return "out of range for this display";
    var s = new SET(); s.header.type = -4; s.header.size = (uint)Marshal.SizeOf(typeof(SET));
    s.header.adapterId = p.source.adapterId; s.header.id = p.source.id; s.scaleRel = rel;
    return "set " + percent + "%: " + DisplayConfigSetDeviceInfo(ref s);
  }
}
'@
if ($Percent -gt 0) { [Dpi]::Set($Percent) }
[Dpi]::Report()
