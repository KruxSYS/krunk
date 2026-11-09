# Krunk 0.10.1: the first unlock from a login form lists only the site's own logins. The
# operator saw every saved password after unlocking: the list offered every login of the
# registrable domain, unlabelled. A fresh profile gets a vault and these logins (saved with
# the "Save password?" prompt):
#   exact-ed, exact-eve  https://login.krunk-test.test:8443   (the site)
#   domain-dan           https://www.krunk-test.test:8443     (same registrable domain)
#   alice                http://localhost:8094                (another site)
# After a restart the vault is locked: the password field on login.krunk-test.test shows
# "Unlock Krunk passwords"; after the unlock the dropdown lists exact-ed and exact-eve only.
# On www.krunk-test.test only domain-dan. Needs serve.ps1 on :8094 and https :8443 (started
# here; the https setup is scripts/vm-050-https-setup.ps1). Screenshots "ul-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'ul'
Copy-Item C:\pipeline\krunk\www\8443\origin-login.html C:\pipeline\krunk\www\8094\origin-login.html -Force
foreach ($prefix in 'http://*:8094/', 'https://*:8443/') {
  $port = $prefix -replace '.*:(\d+)/', '$1'
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',$prefix
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p0101'
$master = 'Krunk-test-master-1'
function StartKrunk([bool]$fresh) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  if ($fresh) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
  $A
}
function Quit { foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }; Start-Sleep 3 }
# The "Save password?" prompt's Save button is the rightmost in its bottom row.
function SaveLogin([string]$url, [string]$user, [string]$pw, [string]$name) {
  SafeGo $url 3500
  Click 170 237 400; Keys '{ESC}' 200; Keys $user 200
  Click 170 315 400; Keys '{ESC}' 200; Keys $pw 200
  Keys '{ENTER}' 2500
  Shot "$P-$name-save-prompt"
  Click 1220 157 2000
}

$A = StartKrunk $true
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-1-vault-created"
SaveLogin 'https://login.krunk-test.test:8443/origin-login.html' 'exact-eve' 'S3cret-eve' '2a'
SaveLogin 'https://login.krunk-test.test:8443/origin-login.html' 'exact-ed' 'S3cret-ed' '2b'
SaveLogin 'https://www.krunk-test.test:8443/origin-login.html' 'domain-dan' 'S3cret-dan' '2c'
SaveLogin 'http://localhost:8094/origin-login.html' 'alice' 'S3cret-alice' '2d'
SafeGo 'krunk://passwords' 3000
Shot "$P-3-logins-saved"
Quit

# Locked after the restart: Unlock from the dropdown, then the list.
$A = StartKrunk $false
SafeGo 'https://login.krunk-test.test:8443/origin-login.html' 3500
Click 170 315 1200
Shot "$P-4-locked-unlock-entry"
Click 190 353 1800
Shot "$P-5-master-prompt"
Keys $master 200; Keys '{ENTER}' 1000
Shot "$P-6-list-right-after-unlock"
Start-Sleep 2
Shot "$P-6b-list-after-unlock-3s"
Keys '{ESC}' 300
# The sister subdomain: only its own login.
SafeGo 'https://www.krunk-test.test:8443/origin-login.html' 3500
Click 170 315 1200
Shot "$P-7-www-own-login-only"
Keys '{ESC}' 300
Quit
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'suggest|fill|login|vault|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line)" }
