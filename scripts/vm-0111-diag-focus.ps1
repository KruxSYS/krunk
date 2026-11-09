# Krunk 0.11.1 diagnosis: does preparing the spare frame (3 s after start) take the focus?
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'diag'
$K = 'C:\pipeline\krunk'
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class GT {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct INFO { public int cb; public int flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT rc; }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint tid, ref INFO i);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  public static string Info(IntPtr anyWindow) {
    uint pid; uint tid = GetWindowThreadProcessId(anyWindow, out pid);
    var i = new INFO(); i.cb = Marshal.SizeOf(typeof(INFO));
    if (!GetGUIThreadInfo(tid, ref i)) return "no info";
    return "active=" + i.active + " focus=" + i.focus + " focusRoot=" + GetAncestor(i.focus, 2);
  }
}
"@
function Describe([string]$label) {
  $A = MainWindow
  $fg = [U]::GetForegroundWindow()
  $info = [GT]::Info($A)
  Log "[$label] A=$A fg=$fg ('$([TW]::Txt($fg))' $([TW]::Cls($fg)) visible $([TW]::IsWindowVisible($fg))); $info"
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  foreach ($h in [TW]::All()) { if ($pids -contains [TW]::Pid($h) -and [TW]::Cls($h) -eq 'KrunkWindow') { Log "[$label] KrunkWindow $h visible $([TW]::IsWindowVisible($h)) '$([TW]::Txt($h))'" } }
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = "$K\p0111f"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 1
$A = MainWindow
Focus $A | Out-Null
Describe 'early'
Start-Sleep 5
Describe 'after-spare'
SafeGo 'krunk://about' 1500
Describe 'after-go'
Shot "$P-1-after-go"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
