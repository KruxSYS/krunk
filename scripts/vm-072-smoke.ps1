# Krunk 0.7.2 final smoke: the installed copy (updated in-app from 0.7.1), on a fresh data folder.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'smoke072'
$K = 'C:\pipeline\krunk'
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Log "[$P] installed: $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk').DisplayVersion); Krunk.dll $((Get-FileHash "$inst\Krunk.dll").Hash.Substring(0,16)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
$profile = "$K\p072s"; Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue; New-Item -ItemType Directory $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { throw 'no focus' }
$c = ClientRect $A
Shot "$P-1-start-ntp"
Keys 'example.com' 200; Keys '{ENTER}' 4000
Shot "$P-2-page"
Log "[$P] title after typing on the start page: $([TW]::Txt($A))"
SafeKeys '^d' 1500; Keys '{ESC}' 500
SafeKeys '^t' 1200; Keys 'exa' 900
Shot "$P-3-new-tab-completion"
Keys '{ESC}' 300; Keys '{ESC}' 300
Click ($c.Right - 20) 56 1200
Shot "$P-4-menu"
Keys '{ESC}' 500
SafeGo 'krunk://about' 3000
Shot "$P-5-about"
SafeGo 'krunk://passwords' 3000
Shot "$P-6-passwords"
SafeGo 'krunk://bookmarks' 3000
Shot "$P-7-bookmarks"
SafeGo 'krunk://settings' 3000
Shot "$P-8-settings"
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'starting|engine files|update check|WARN|ERROR') -join ' ## ')"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Log "[$P] done"
