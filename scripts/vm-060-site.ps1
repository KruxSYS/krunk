# Krunk 0.6.0 milestone 16 (R27): the website detects Krunk. A dev build whose update base is
# http://localhost:8099 (UPDATE_BASE_URL + dev-updates) opens the staged site through an SSH
# reverse tunnel (ssh -R 8099:dind:<staging port>): the download box says "You're using Krunk
# <version>", the same page from another origin (127.0.0.1) and the privacy page show nothing,
# and the page's own probe shows `window.krunk` frozen on the site origin only.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "site-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p060s'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
try { $cfg = Invoke-WebRequest -UseBasicParsing 'http://localhost:8099/api/v1/web/config' -TimeoutSec 10; Log "[$P] tunnel: web/config $($cfg.StatusCode) $($cfg.Content)" } catch { Log "[$P] tunnel: $_" }
SafeGo 'http://localhost:8099/' 6000
Shot "$P-1-site-in-krunk"
SafeGo 'http://127.0.0.1:8099/' 6000
Shot "$P-2-other-origin-no-marker"
SafeGo 'http://localhost:8099/privacy' 4000
Shot "$P-3-privacy"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'update check|WARN|ERROR' | Select-Object -Last 6 | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
