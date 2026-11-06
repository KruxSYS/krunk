# Krunk 0.5.0 milestone 9: import from other browsers. Two phases:
#   -Phase browsers   drive real Firefox and Chrome (installed by scripts/vm.sh sh with
#                     install-browsers.ps1) once: visit pages, save a fake login each through their
#                     own "Save password" prompt, add a bookmark, quit. Edge already has a profile.
#   -Phase krunk      Krunk on the real %LOCALAPPDATA% (its folder is set aside and restored): a new
#                     vault, Settings → Passwords & import: the detected profiles, an import from each
#                     browser, the CSV import, a second import (nothing new), the results on
#                     krunk://passwords, krunk://history and the bookmarks bar.
# Screenshots "im-*". All logins are fake.
param([string]$Phase = 'krunk')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'im'
foreach ($port in 8094) {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',"http://*:$port/"
    Start-Sleep 2
  }
}
Copy-Item C:\pipeline\krunk\www\8443\origin-login.html C:\pipeline\krunk\www\8094\origin-login.html -Force

function WaitWindow([string]$class, [int]$tries = 60) {
  for ($i = 0; $i -lt $tries; $i++) {
    $h = [U]::FindWindow($class, [NullString]::Value)
    if ($h -ne [IntPtr]::Zero) { return $h }
    Start-Sleep -Milliseconds 500
  }
  [IntPtr]::Zero
}
function Place([IntPtr]$h) { [U]::MoveWindow($h, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $h | Out-Null }

if ($Phase -eq 'browsers') {
  # ---- Firefox: first run makes the default profile under %APPDATA%\Mozilla\Firefox.
  Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Start-Process 'C:\Program Files\Mozilla Firefox\firefox.exe' -ArgumentList 'http://localhost:8094/origin-login.html'
  $F = WaitWindow 'MozillaWindowClass'; Start-Sleep 8; Place $F; Start-Sleep 2; Place $F
  Shot "$P-b1-firefox-first-run"
  Click 640 326 1500   # "Continue" of the terms dialog, if the policy did not skip it
  Keys '^l' 300; Keys 'http://localhost:8094/origin-login.html' 100; Keys '{ENTER}' 3000
  Click 600 450 300; Keys '{TAB}' 200; Keys 'ff-user' 200; Keys '{TAB}' 200; Keys 'Fx-real-pass-1' 200; Keys '{ENTER}' 3000
  Shot "$P-b2-firefox-save-prompt"
  Click 508 260 1500   # the doorhanger's Save button
  Shot "$P-b3-firefox-saved"
  Keys '^l' 300; Keys 'http://localhost:8094/links.html' 100; Keys '{ENTER}' 3000
  Keys '^d' 1500; Keys '{ENTER}' 1500
  Shot "$P-b4-firefox-bookmarked"
  Keys '^l' 300; Keys 'http://localhost:8094/media.html' 100; Keys '{ENTER}' 3000
  Keys '^+q' 3000
  Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force
  # ---- Chrome: the default profile under %LOCALAPPDATA%\Google\Chrome\User Data.
  Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Start-Process 'C:\Program Files\Google\Chrome\Application\chrome.exe' -ArgumentList '--no-first-run','--no-default-browser-check','--disable-sync','http://localhost:8094/origin-login.html'
  # Chrome has several windows of that class; its own placement (18,10 1033x725) is kept.
  $C = WaitWindow 'Chrome_WidgetWin_1'; Start-Sleep 8
  Shot "$P-b5-chrome-first-run"
  Keys '^l' 300; Keys 'http://localhost:8094/origin-login.html' 100; Keys '{ENTER}' 3000
  Click 600 450 300; Keys '{TAB}' 200; Keys 'cr-user' 200; Keys '{TAB}' 200; Keys 'Cr-real-pass-1' 200; Keys '{ENTER}' 3000
  Shot "$P-b6-chrome-save-prompt"
  Click 798 382 1500   # the bubble's Save button
  Shot "$P-b7-chrome-saved"
  Keys '^l' 300; Keys 'http://localhost:8094/links.html' 100; Keys '{ENTER}' 3000
  Keys '^d' 1500; Keys '{ENTER}' 1500
  Shot "$P-b8-chrome-bookmarked"
  Keys '^l' 300; Keys 'http://localhost:8094/media.html' 100; Keys '{ENTER}' 3000
  Keys '%{F4}' 3000
  Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -Force
  Log "[browsers] firefox profiles: $(Get-Content "$env:APPDATA\Mozilla\Firefox\profiles.ini" -ErrorAction SilentlyContinue | Out-String)"
  Log "[browsers] chrome Login Data: $(Test-Path "$env:LOCALAPPDATA\Google\Chrome\User Data\Default\Login Data")"
  return
}

# ---- Krunk on the real %LOCALAPPDATA% (set aside and restored).
$master = 'Krunk-test-master-1'
$real = "$env:LOCALAPPDATA\Krunk"
$aside = "$env:LOCALAPPDATA\Krunk.before-import-test"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
if (Test-Path $aside) { Remove-Item -Recurse -Force $real -ErrorAction SilentlyContinue } elseif (Test-Path $real) { Rename-Item $real $aside }
try {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  $A = WaitWindow 'KrunkWindow'; Start-Sleep 6; Place $A
  SafeGo 'krunk://passwords' 3500
  Click 640 371 300; Keys $master 200
  Click 640 438 300; Keys $master 200
  Click 456 551 4000
  SafeGo 'krunk://settings#passwords' 4000
  Shot "$P-1-profiles-detected"
  Log "[krunk] profiles shot taken; the import clicks follow the layout seen there"
  KrunkWindows $P
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
  Copy-Item "$real\krunk.log" "C:\pipeline\krunk\shots\im-krunk.log" -Force -ErrorAction SilentlyContinue
} finally {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Remove-Item -Recurse -Force $real -ErrorAction SilentlyContinue
  if (Test-Path $aside) { Rename-Item $aside $real }
}
