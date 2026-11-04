# Final end-to-end pass on the VM (scripts/vm.sh ps1; needs lib.ps1, vm-install.ps1, the
# installer as C:\pipeline\krunk\Krunk-Setup.exe and serve.ps1 on localhost:8090).
# Screenshots with the prefix "final-" and lines in run.log.
. C:\pipeline\krunk\lib.ps1
$P = 'final'
# 1. Silent install over the running Krunk, then start from the Start Menu.
Log "[$P] running before install: $((Get-Process Krunk -EA SilentlyContinue | Measure).Count) processes"
$t0 = Get-Date
$setup = Start-Process C:\pipeline\krunk\Krunk-Setup.exe -ArgumentList "/S" -PassThru; $setup.WaitForExit(240000) | Out-Null
Log "[$P] silent install done in $([int]((Get-Date) - $t0).TotalSeconds) s; running after: $((Get-Process Krunk -EA SilentlyContinue | Measure).Count)"
$hash = (Get-FileHash "$env:LOCALAPPDATA\Programs\Krunk\Krunk.dll").Hash.ToLower()
Log "[$P] installed Krunk.dll sha256 $hash"
Start-Process (Join-Path ([Environment]::GetFolderPath('Programs')) 'Krunk.lnk')
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$h = MainWindow; [U]::ShowWindow($h, 3) | Out-Null; Start-Sleep 1
Click 640 15 400
Shot "$P-01-started"
# 2. New tab: typing goes to the address bar.
Keys '^t' 1500; Keys 'example.com' 600
Shot "$P-02-newtab-typing"
Keys '{ENTER}' 3000
Shot "$P-03-example"
# 3. Storage page.
Go 'krunk://storage' 2500
Shot "$P-04-storage"
# 4. Settings -> About.
Go 'krunk://settings' 2500
Keys '^{END}' 1000
Shot "$P-05-settings-about"
# 5. Pointer lock on the test page.
Go 'http://localhost:8090/pointer-lock.html' 2500
Click 640 300 1200
Shot "$P-06-locked"
RelMove 25 0 60 8
RelMove 0 15 40 8
Start-Sleep -Milliseconds 600
Shot "$P-07-locked-moved"
Keys '{ESC}' 1000
Shot "$P-08-esc-unlocked"
# 6. F11 browser full screen.
Keys '{F11}' 1500
Shot "$P-09-f11"
Keys '{F11}' 1500
Shot "$P-10-f11-off"
Get-Content "$env:LOCALAPPDATA\Krunk\krunk.log" | Select-String 'starting|pointer|full screen|update check' | Select-Object -Last 10 | ForEach-Object { Log "[$P] log: $_" }
