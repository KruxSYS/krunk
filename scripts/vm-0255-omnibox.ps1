# Krunk 0.25.5 VM check: address-bar suggestions latency. Starts the fast build
# (C:\pipeline\krunk\app255, profile p0254), types a first letter into the bar several times
# and copies the log lines that time the dropdown to $Shots\omni.log.
. C:\pipeline\krunk\lib.ps1
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0254"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
New-Item -ItemType Directory -Force $prof | Out-Null
$env:LOCALAPPDATA = $prof
Start-Process -FilePath "$Root\app255\Krunk.exe" -WorkingDirectory "$Root\app255"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
Start-Sleep 22
Click 1127 175 500   # a fast build's "repair" prompt: Later
Click 640 400 500
Go 'https://example.com/' 3000
Go 'https://www.wikipedia.org/' 4000
foreach ($c in 'e','w','k','x','e','w') {
  Keys '^l' 400; Keys $c 900
  if ($c -eq 'w') { Shot 'v0255-omni' }
  Keys '{ESC}' 200; Keys '{ESC}' 300
}
$log = Get-ChildItem -Recurse -Filter krunk.log $prof | Sort-Object LastWriteTime | Select-Object -Last 1
Select-String -Path $log.FullName -Pattern 'address bar|paint answer' | ForEach-Object { $_.Line } | Set-Content "$Shots\omni.log"
Log "omni done ($($log.FullName))"
