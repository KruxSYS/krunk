# Probe: krunk://extensions in a first and a second tab of the profile p060x (extensions installed).
param([string]$Phase = 'probe')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "ext-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p060x'
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
SafeGo 'krunk://extensions' 3500
Shot "$P-1-tab1"
SafeKeys '^t' 1500; SafeKeys 'krunk://extensions' 100; SafeKeys '{ENTER}' 3500
Shot "$P-2-tab2"
Keys '{F5}' 3000
Shot "$P-3-tab2-reload"
SafeKeys '^t' 1500; SafeKeys 'http://localhost:8094/ext.html' 100; SafeKeys '{ENTER}' 3500
SafeGo 'krunk://extensions' 3500
Shot "$P-4-tab3-after-page"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'asked for the list|rejected|WARN' | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
