# Krunk 0.4.0 release check: the sandbox of every process (vm-sandbox.ps1) and the process
# tree's memory (vm-mem.ps1) with a few pages open, then exit with no process left.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p040s'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
if (-not (Focus $A)) { throw 'no focus' }
SafeGo 'https://example.com/' 3000
SafeKeys '^t' 1000; SafeGo 'http://localhost:8092/ads.html' 3000
SafeKeys '^t' 1000; SafeGo 'krunk://adblock' 2500
Shot 'sbx-pages'
& powershell -NoProfile -ExecutionPolicy Bypass -File C:\pipeline\krunk\vm-sandbox.ps1 | ForEach-Object { Log "[sandbox] $_" }
& powershell -NoProfile -ExecutionPolicy Bypass -File C:\pipeline\krunk\vm-mem.ps1 | ForEach-Object { Log "[mem] $_" }
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
for ($i = 0; $i -lt 20 -and (Get-Process Krunk -EA SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | ForEach-Object { Log "[log] $_" }
Log "[log] WARN/ERROR lines: $(@(Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR').Count)"
