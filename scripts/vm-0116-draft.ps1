# Krunk 0.11.6: text typed into the address bar stays with its tab across a tab switch.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'draft'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$profile = "$K\p0116"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'https://example.com/' 2500
SafeKeys '^t' 1500
SafeKeys 'half typ' 800
$t = TabPoint $A 0 2
Click $t[0] $t[1] 1200                     # the first tab
Shot "$P-1-other-tab"
$t = TabPoint $A 1 2
Click $t[0] $t[1] 1200                     # back to the new tab
Shot "$P-2-back"
SafeKeys '^{TAB}' 1000                     # keyboard switch while the text is there
SafeKeys '^{TAB}' 1000
Shot "$P-3-ctrl-tab-back"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
