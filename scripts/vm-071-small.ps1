# Krunk 0.7.1 M5: About shows the engine's command line, krunk.log names it, and a wrong master
# password stays said on the unlock form after the delay (test password only).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'small071'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = "$K\p071s"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'krunk://about' 3000
Shot "$P-1-about"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String "Krunk's switches|engine command line") -join ' ## ')"
SafeGo 'krunk://passwords' 3000
TypeSecret 'Krunk-071-test-pw'; Keys '{TAB}' 200; TypeSecret 'Krunk-071-test-pw'; Keys '{ENTER}' 4000
Click 1069 145 1500                                 # Lock
TypeSecret 'not-the-password'; Keys '{ENTER}' 1000
Shot "$P-2-wrong-during-delay"
Start-Sleep 4
Shot "$P-3-wrong-after-delay"
Log "[$P] vault log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'vault') -join ' ## ')"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
