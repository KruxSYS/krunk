# Krunk 0.28.1 end-to-end: the main browser flows on the running build (after start.ps1), one shot each.
. C:\pipeline\krunk\lib.ps1
$S = 'C:\pipeline\krunk\shots'
function ShotAs([string]$n) { Shot "e2e-$n" }
Start-Sleep 3; ShotAs '01-newtab'
Click 640 30 300; Go 'https://example.com/' 5000; ShotAs '02-page'
Keys '^d' 2500; ShotAs '03-bookmark-dialog'; Keys '{ENTER}' 1500
Keys '^t' 2500; ShotAs '04-new-tab-with-bookmark'
Click 640 30 300; Go 'http://127.0.0.1:18111/sample-download.zip' 6000; ShotAs '05-download'
Keys '^j' 3000; ShotAs '06-downloads-page'
Click 640 30 300; Go 'krunk://settings' 3000; ShotAs '07-settings'
$log = Join-Path $env:LOCALAPPDATA 'Krunk\krunk.log'
Select-String -Path $log -Pattern 'update check|release files|bookmark|download' | Select-Object -Last 10 | ForEach-Object { Log "[e2e] $($_.Line)" }
