# XML viewer before/after (2026-11-24): the installed Krunk and the test build in C:\pipeline\krunk\appxml
# open each URL as a launch argument (no typing: another app on the shared VM can take the focus)
# and the Krunk window itself is captured with PrintWindow; shots xmlw-<installed|fixed>-<page>.png.
. C:\pipeline\krunk\lib.ps1
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class PW { [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f); }
"@
$K = 'C:\pipeline\krunk'
$runs = @(
  @{ exe = "$env:LOCALAPPDATA\Programs\Krunk\Krunk.exe"; tag = 'installed' },
  @{ exe = "$K\appxml\Krunk.exe"; tag = 'fixed' })
$urls = @{ 'sitemap' = 'https://kruxsys.com/sitemap.xml'; 'note' = 'https://www.w3schools.com/xml/note.xml'; 'html-strict-csp' = 'https://kruxsys.com/' }
$base = $env:LOCALAPPDATA
foreach ($r in $runs) {
  foreach ($u in $urls.GetEnumerator()) {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
    $prof = "$K\lp-xml"; Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" $prof
    $env:LOCALAPPDATA = $prof
    Start-Process -FilePath $r.exe -WorkingDirectory (Split-Path $r.exe) -ArgumentList $u.Value
    $env:LOCALAPPDATA = $base
    for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 9
    $bmp = New-Object System.Drawing.Bitmap 1280, 760; $g = [System.Drawing.Graphics]::FromImage($bmp)
    $dc = $g.GetHdc(); [PW]::PrintWindow($A, $dc, 2) | Out-Null; $g.ReleaseHdc($dc); $g.Dispose()
    $bmp.Save("$Shots\xmlw-$($r.tag)-$($u.Key).png"); $bmp.Dispose()
    Log "xmlw $($r.tag) $($u.Key) $((Get-Item $r.exe).VersionInfo.ProductVersion)"
  }
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
