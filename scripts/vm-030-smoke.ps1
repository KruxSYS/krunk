# 0.3.0 single-window smoke test (the multi-window refactor must not change behaviour):
# tabs, menu and shield popups, find, zoom, full screen, pointer lock, exit without stray
# processes, session restore. Needs serve.ps1 on localhost:8090. Prefix "smoke-".
param([string]$P = 'smoke')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
function Start-Krunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $h = MainWindow; [U]::ShowWindow($h, 3) | Out-Null; Start-Sleep 1
  Click 640 15 400
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8090*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8090/'
}
Remove-Item "$env:LOCALAPPDATA\Krunk\krunk.log" -ErrorAction SilentlyContinue
Start-Krunk
Shot "$P-01-started"
Keys '^t' 1500; Keys 'example.com' 500; Keys '{ENTER}' 3000
Shot "$P-02-example"
Keys '^t' 1500; Go 'krunk://settings' 2500
Keys '^{TAB}' 1000
Shot "$P-03-cycled"
$c = ClientRect $h; $h = MainWindow; $c = ClientRect $h
Click ($c.Right - 22) 56 1200
Shot "$P-04-menu"
Keys '{ESC}' 600
Click ($c.Right - 56) 56 1200
Shot "$P-05-shield"
Keys '{ESC}' 600
Click 640 400 300
Keys '^f' 600; Keys 'domain' 800
Shot "$P-06-find"
Keys '{ESC}' 600
Keys '^{+}' 400; Keys '^{+}' 800
Shot "$P-07-zoom"
Keys '^0' 600
Keys '{F11}' 1500
Shot "$P-08-f11"
Keys '{F11}' 1500
Go 'http://localhost:8090/pointer-lock.html' 2500
Click 640 400 1200
RelMove 25 0 40 8
Start-Sleep -Milliseconds 600
Shot "$P-09-locked"
Keys '{ESC}' 1000
KrunkWindows "$P"
# Exit: close the window; no Krunk process may remain.
[U]::SetForegroundWindow((MainWindow)) | Out-Null
Get-Process Krunk -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 6
Log "[$P] processes after closing the window: $((Get-Process Krunk -ErrorAction SilentlyContinue | Measure-Object).Count)"
Start-Krunk
Start-Sleep 2
Shot "$P-10-restored"
KrunkWindows "$P-restored"
Get-Content "$env:LOCALAPPDATA\Krunk\krunk.log" | Select-String 'WARN|ERROR|PANIC|window|full screen|pointer' | ForEach-Object { Log "[$P] log: $_" }
