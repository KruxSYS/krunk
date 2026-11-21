# Krunk 0.26.0: does an installed 0.25.5 find the live 0.26.0 update? Installs -Setup (0.25.5) with
# /S, starts the installed copy with its normal profile, waits for the update check, opens the menu
# (0.7.2: a dot on the menu button, the first item leads to the update) and takes the shot
# update-found. Logs the installed version and the krunk.log update lines.
param([string]$Setup = 'C:\pipeline\krunk\Krunk-Setup-0.25.5.exe')
. C:\pipeline\krunk\lib.ps1
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$data = Join-Path $env:LOCALAPPDATA 'Krunk'
Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$proc = Start-Process -FilePath $Setup -ArgumentList '/S' -PassThru
$proc.WaitForExit(180000) | Out-Null
Start-Sleep 3
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$ver = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion
Log "[update-found] installed $ver"
Remove-Item Env:LOCALAPPDATA_OVERRIDE -ErrorAction SilentlyContinue
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
Start-Sleep 25
[U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A); [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
Start-Sleep 1
Shot 'update-dot'
Click 1260 56 1500
Shot 'update-found'
$lines = Select-String -Path "$data\krunk.log" -Pattern 'update' -ErrorAction SilentlyContinue | Select-Object -Last 6 | ForEach-Object { $_.Line }
Log "[update-found] log: $($lines -join ' | ')"
Keys '{ESC}' 300
