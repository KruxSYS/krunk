# Krunk 0.8.0 (M6b): a site's HTTP sign-in (Basic) with the vault. serve.ps1's /auth/* wants
# tester / s3cret-pass (realm "Krunk test area"). The dialog, Esc (the 401 page), a wrong password
# ("not accepted"), the right one (signed in, Save password?), then after a restart (the engine's
# credential cache is gone, the vault locked): unlock from the dialog, the saved login filled,
# one click to sign in. The log must hold no user name, password or realm.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'auth080'
$K = 'C:\pipeline\krunk'
$App = "$K\app"
$master = 'Krunk-test-master-1'
$url = 'http://localhost:8094/auth/area.html'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$logStart = (Get-Content "$K\www\access.log").Count
$profile = "$K\p080h"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
}
function Title { [TW]::Txt((MainWindow)) }
function Dialog {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  foreach ($h in [TW]::All()) {
    if ($pids -contains [TW]::Pid($h) -and [TW]::IsWindowVisible($h) -and [TW]::Cls($h) -eq 'KrunkPopup') { $r = WindowRect $h; return "dialog $($r.Left),$($r.Top) $($r.Right - $r.Left)x$($r.Bottom - $r.Top)" }
  }
  'no dialog'
}
StartKrunk
# A vault (as in vm-072-ui).
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
SafeGo $url 2500
Log "[$P] 1 challenge: '$(Title)' $(Dialog)"
Shot "$P-1-dialog"
Keys '{ESC}' 1500
Log "[$P] 2 Esc: '$(Title)' $(Dialog)"
Shot "$P-2-after-esc"
SafeKeys '{F5}' 2500
Log "[$P] 3 reload: $(Dialog)"
Keys 'tester' 150; Keys '{TAB}' 150; Keys 'wrong-pass' 150; Keys '{ENTER}' 2500
Log "[$P] 4 wrong password: '$(Title)' $(Dialog)"
Shot "$P-4-not-accepted"
Keys 'tester' 150; Keys '{TAB}' 150; Keys 's3cret-pass' 150; Keys '{ENTER}' 3000
Log "[$P] 5 right password: '$(Title)' $(Dialog)"
Shot "$P-5-signed-in-save-prompt"
Keys '{ENTER}' 2000
Log "[$P] 6 saved: $(Dialog)"
Shot "$P-6-saved"
if (Focus $A) { SafeKeys '%{F4}' 2500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
# After a restart: the vault is locked, the engine forgot the credentials.
StartKrunk
SafeGo $url 2500
Log "[$P] 7 after restart: $(Dialog)"
Shot "$P-7-locked"
Keys '+{TAB}' 200; Keys '{ENTER}' 1500
Log "[$P] 8 unlock: $(Dialog)"
Shot "$P-8-unlock"
Keys $master 200; Keys '{ENTER}' 3500
Log "[$P] 9 unlocked: $(Dialog)"
Shot "$P-9-filled"
Keys '{ENTER}' 3000
Log "[$P] 10 one click: '$(Title)' $(Dialog)"
Shot "$P-10-signed-in-with-saved"
if (Focus $A) { SafeKeys '%{F4}' 2500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Get-Content "$profile\Krunk\krunk.log"
Log "[$P] http auth lines: $(($log | Select-String 'http auth' | ForEach-Object { $_.Line }) -join ' ## ')"
Log "[$P] secrets in krunk.log: $(($log | Select-String -SimpleMatch 'tester','s3cret','wrong-pass','Krunk test area' | Measure-Object).Count)"
Log "[$P] WARN/ERROR: $(($log | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(($log | Select-String ' WARN | ERROR ' | ForEach-Object { $_.Line }) -join ' ## ')"
Log "[$P] server: $((Get-Content "$K\www\access.log" | Select-Object -Skip $logStart | Select-String ' auth ' | ForEach-Object { ($_.Line -split ' ')[1..2] -join ' ' }) -join ', ')"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
Log "[$P] done"
