# XML viewer check (2026-11-24): the installed Krunk (or -Exe) with a fresh copy of lp-base opens
# a sitemap with a strict CSP, an XML file without one, and an HTML page with the strict CSP;
# screenshots xml-<Tag>-*. vm.sh ps1 passes no arguments: edit the param defaults (or sed them).
param([string]$Exe = '', [string]$Tag = 'installed')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'
if (-not $Exe) { $Exe = "$env:LOCALAPPDATA\Programs\Krunk\Krunk.exe" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$prof = "$K\lp-xml"
Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" $prof
$env:LOCALAPPDATA = $prof
Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe)
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 6
[U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
[U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 800
Click 640 30 300
Log "xml $Tag exe=$Exe version=$((Get-Item $Exe).VersionInfo.ProductVersion)"
Go 'https://kruxsys.com/sitemap.xml' 6000; Shot "xml-$Tag-sitemap"
Go 'https://www.w3schools.com/xml/note.xml' 6000; Shot "xml-$Tag-note"
Go 'https://kruxsys.com/' 6000; Shot "xml-$Tag-html-strict-csp"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "xml $Tag done"
