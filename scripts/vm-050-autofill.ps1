# Krunk 0.5.0 milestone 8: login suggestions under a focused field. A fresh profile gets a
# vault (krunk://passwords) and logins saved the normal way (the "Save password?" prompt):
#   alice      http://localhost:8094            the test page's top-level form
#   framebob   http://127.0.0.1:8095            the origin of the page's cross-origin iframe
#   exact-eve  https://login.krunk-test.test:8443   (exact https origin)
#   domain-dan https://www.krunk-test.test:8443     (same registrable domain, other host)
#   http-hank  http://login.krunk-test.test:8094    (same host, plain http)
# Before any login exists, focusing a field shows nothing. After a restart the vault is locked:
# focusing the password field shows "Unlock Krunk passwords", the click opens Krunk's
# master-password prompt, a wrong password fails cleanly, the right one fills the form (one
# match). The next focus shows the username with no prompt. The iframe's field shows only the
# iframe's own login. On https://login.krunk-test.test the exact entry comes first, then the
# domain match (with its host); the http entry is never offered to https, and http gets only
# its own. A private tab gets suggestions and fills, but never offers to save. The pages print
# their fields and every input/change event, so the screenshots show when the password
# arrived. Needs serve.ps1 on :8094, :8095 and https :8443 (started here; the https setup is
# scripts/vm-050-https-setup.ps1). Screenshots "af-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'af'
Copy-Item C:\pipeline\krunk\www\8443\origin-login.html C:\pipeline\krunk\www\8094\origin-login.html -Force
foreach ($prefix in 'http://*:8094/', 'http://*:8095/', 'https://*:8443/') {
  $port = $prefix -replace '.*:(\d+)/', '$1'
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',$prefix
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p050a'
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
# The "Save password?" prompt sits under the right end of the toolbar; its Save button is the
# rightmost in the bottom row.
function SaveLogin([string]$url, [int]$ux, [int]$uy, [int]$px, [int]$py, [string]$user, [string]$pw, [string]$name) {
  SafeGo $url 3500
  Click $ux $uy 400; Keys $user 200
  Click $px $py 400; Keys $pw 200
  Keys '{ENTER}' 2500
  Shot "$P-$name-save-prompt"
  Click 1220 157 2000
}

$A = StartKrunk $true
# (1) The vault.
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-1-vault-created"
# (2) No logins yet: focusing a field shows nothing.
SafeGo 'http://localhost:8094/login.html' 3500
Click 170 315 1200
Shot "$P-2-no-logins-no-dropdown"
Keys '{ESC}' 300
# (3) The logins, saved with the prompt.
SaveLogin 'http://localhost:8094/login.html' 170 237 170 315 'alice' 'S3cret-alice' '3-top'
SaveLogin 'http://127.0.0.1:8095/frame-login.html' 150 177 150 245 'framebob' 'S3cret-frame' '4-frame-origin'
SaveLogin 'https://login.krunk-test.test:8443/origin-login.html' 170 237 170 315 'exact-eve' 'S3cret-exact' '4b-https-exact'
SaveLogin 'https://www.krunk-test.test:8443/origin-login.html' 170 237 170 315 'domain-dan' 'S3cret-domain' '4c-https-domain'
SaveLogin 'http://login.krunk-test.test:8094/origin-login.html' 170 237 170 315 'http-hank' 'S3cret-http' '4d-http-host'
SafeGo 'krunk://passwords' 3000
Shot "$P-5-logins-saved"
Quit

# (4) Locked after a restart: the Unlock entry, Krunk's prompt, a wrong password, the fill.
$A = StartKrunk $false
SafeGo 'http://localhost:8094/login.html' 3500
Click 170 315 1200
Shot "$P-6-locked-unlock-entry"
Click 190 353 1800
Shot "$P-7-master-prompt"
Keys 'not-the-master-password' 200; Keys '{ENTER}' 2500
Shot "$P-7b-wrong-master-password"
Keys $master 200; Keys '{ENTER}' 3000
Shot "$P-8-filled-after-unlock"
# (5) The second time: the username, no prompt.
SafeGo 'http://localhost:8094/login.html' 3500
Click 170 237 1200
Shot "$P-9-suggestion-no-prompt"
Click 190 276 1500
Shot "$P-10-filled-from-suggestion"
# (6) The iframe: only its own origin's login.
Click 175 630 1200
Shot "$P-11-iframe-suggestion"
Click 305 665 1500
Shot "$P-12-iframe-filled"
# (7) https: the exact origin first, then the registrable-domain match; never the http entry.
SafeGo 'https://login.krunk-test.test:8443/origin-login.html' 3500
Click 170 315 1200
Shot "$P-13-https-exact-then-domain"
Click 190 391 1500
Shot "$P-14-https-domain-entry-filled"
# (8) http on the same host: only the http entry.
SafeGo 'http://login.krunk-test.test:8094/origin-login.html' 3500
Click 170 237 1200
Shot "$P-15-http-only-own-entry"
Keys '{ESC}' 300
# (9) A private tab: suggestions and the fill work; a new login is never offered for saving.
SafeKeys '^+n' 2500
SafeGo 'http://localhost:8094/login.html' 3500
Click 170 315 1200
Shot "$P-16-private-tab-suggestion"
Click 190 353 1500
Shot "$P-17-private-tab-filled"
SafeGo 'http://localhost:8094/login.html' 3500
Click 170 237 400; Keys '{ESC}' 300; Keys 'privpat' 200
Click 170 315 400; Keys '{ESC}' 300; Keys 'S3cret-private' 200
Keys '{ENTER}' 2500
Shot "$P-18-private-tab-no-save-prompt"
KrunkWindows $P
Quit
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'suggest|fill|login|vault|engine pref|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line)" }
