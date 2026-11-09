# Krunk 0.11.3 probe: what a PDF (inline, then a file:// copy) and a mailto: link do today.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'probe'
$K = 'C:\pipeline\krunk'
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$profile = "$K\p0112p"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'http://localhost:8094/test.pdf' 4000
Shot "$P-1-inline-pdf"
Copy-Item "$K\www\8094\test.pdf" "$env:TEMP\krunk-probe.pdf" -Force
SafeGo "file:///$($env:TEMP -replace '\\','/')/krunk-probe.pdf" 4000
Shot "$P-2-file-pdf"
SafeGo 'http://localhost:8094/mail.html' 2500
Click 60 120 2500
Shot "$P-3-after-mailto"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'download|pdf|mailto|WARN|ERROR|refused|blocked|external' | ForEach-Object { Log "[log] $_" }
Get-ChildItem "$profile" -Recurse -Filter *.pdf -ErrorAction SilentlyContinue | ForEach-Object { Log "[pdf file] $($_.FullName)" }
Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'test*.pdf' -ErrorAction SilentlyContinue | ForEach-Object { Log "[downloads] $($_.FullName) $($_.LastWriteTime)" }
Log "[$P] done"
