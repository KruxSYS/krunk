# Krunk 0.27.0 default identity check (latest Chrome): a fresh copy of lp-base (user agent
# Default), then a header echo page (User-Agent, Sec-CH-UA) and a DataDome-protected site that
# turned the 0.26.0 identity away. -App picks the build (app260rel: before, app270: after);
# shots ua-<Tag>-headers / ua-<Tag>-egift.
param([string]$App = 'app270', [string]$Tag = 'new')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$P = "lp-ua-$Tag"
Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$P"
$env:LOCALAPPDATA = "$K\$P"
Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 6
[U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
[U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 800
Click 640 30 300
Go 'https://httpbin.org/headers' 6000; Shot "ua-$Tag-headers"
# An unpublished build of a published version gets the Repair popup: Later, back to the frame.
Click 1127 193 800; Click 640 30 300
Go 'https://egift.activationspot.com/' 15000; Shot "ua-$Tag-egift"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 4; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
