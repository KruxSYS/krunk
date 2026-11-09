. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'; $P = 'hp'; $profile = "$K\p0110h"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $A | Out-Null
SafeGo 'http://localhost:8094/hang.html' 3500
$t = Get-Date
Click 640 400 300; Keys 'abc' 300
for ($i = 0; $i -lt 30; $i++) { Start-Sleep 1; if ((Get-Content "$profile\Krunk\krunk.log" | Select-String 'not responding').Count) { break } }
Log "[$P] after $([int]((Get-Date) - $t).TotalSeconds) s: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'respond' ) -join ' ## ')"
Shot "$P-prompt"
Log "[$P] cef.log: $((Get-Content "$profile\Krunk\cef.log" -ErrorAction SilentlyContinue | Select-Object -Last 8) -join ' ## ')"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
