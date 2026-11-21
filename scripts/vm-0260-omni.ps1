# Krunk 0.26.0 regression: the 0.25.5 address-bar latency check (vm-0255-omnibox.ps1) for any
# build (-App, fresh copy of lp-base as the profile, -Tag names the outputs): types a first
# letter into the bar several times and copies the timing log lines to $Shots\omni-TAG.log.
param([string]$App = 'app260rel', [string]$Tag = 'new')
. C:\pipeline\krunk\lib.ps1
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0260-omni-$Tag"; Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; Copy-Item -Recurse "$Root\lp-base" $prof
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
New-Item -ItemType Directory -Force $prof | Out-Null
$env:LOCALAPPDATA = $prof
Start-Process -FilePath "$Root\$App\Krunk.exe" -WorkingDirectory "$Root\$App"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
Start-Sleep 22
Click 1127 175 500   # a fast build's "repair" prompt: Later
Click 640 400 500
Go 'https://example.com/' 3000
Go 'https://www.wikipedia.org/' 4000
foreach ($c in 'e','w','k','x','e','w') {
  Keys '^l' 400; Keys $c 900
  if ($c -eq 'w') { Shot "omni-$Tag" }
  Keys '{ESC}' 200; Keys '{ESC}' 300
}
$log = Get-ChildItem -Recurse -Filter krunk.log $prof | Sort-Object LastWriteTime | Select-Object -Last 1
Select-String -Path $log.FullName -Pattern 'address bar|paint answer' | ForEach-Object { $_.Line } | Set-Content "$Shots\omni-$Tag.log"
Log "omni done ($($log.FullName))"
