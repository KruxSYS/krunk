# Krunk 0.7.2 M7 (R12): Firefox passwords import. The VM's Firefox 159 profile gets two more fake
# logins saved through Firefox's own prompt (3 in all; logins.json and key4.db are backed up and
# put back at the end), and the firefox-twokeys fixture (an old 3DES key and the newer AES key,
# 3 logins) is registered as a second Firefox profile with a copy of a fixture places.sqlite.
# Krunk (fresh data folder in place of the real one, restored afterwards) makes a vault and imports
# from each profile, then once more from the first (nothing new). The import log lines and the
# number of entries in each profile's logins.json are written to run.log. All logins are fake.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'im072'
$K = 'C:\pipeline\krunk'
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Copy-Item "$K\www\8443\origin-login.html" "$K\www\8094\origin-login.html" -Force
function WaitWindow([string]$class, [int]$tries = 60) {
  for ($i = 0; $i -lt $tries; $i++) { $h = [U]::FindWindow($class, [NullString]::Value); if ($h -ne [IntPtr]::Zero) { return $h }; Start-Sleep -Milliseconds 500 }
  [IntPtr]::Zero
}
function Place([IntPtr]$h) { [U]::MoveWindow($h, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $h | Out-Null }
$ffRoot = "$env:APPDATA\Mozilla\Firefox"
$ffProfile = Get-ChildItem "$ffRoot\Profiles" -Directory | Where-Object { $_.Name -like '*.default-release' } | Select-Object -First 1
$bak = "$K\ff072-backup"
Remove-Item -Recurse -Force $bak -ErrorAction SilentlyContinue; New-Item -ItemType Directory $bak | Out-Null
foreach ($f in 'logins.json', 'key4.db', 'logins-backup.json') { if (Test-Path "$($ffProfile.FullName)\$f") { Copy-Item "$($ffProfile.FullName)\$f" $bak } }
Copy-Item "$ffRoot\profiles.ini" "$bak\profiles.ini"
$twoDir = "$ffRoot\Profiles\krunk072.twokeys"
function Entries([string]$dir) { (Get-Content "$dir\logins.json" -Raw | ConvertFrom-Json).logins.Count }
$master = 'Krunk-test-master-1'
$real = "$env:LOCALAPPDATA\Krunk"
$aside = "$env:LOCALAPPDATA\Krunk.before-import-test"
try {
  # ---- Firefox: two more logins through its own Save prompt.
  Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Log "[$P] Firefox profile $($ffProfile.Name): $(Entries $ffProfile.FullName) login(s) before"
  Start-Process 'C:\Program Files\Mozilla Firefox\firefox.exe' -ArgumentList 'http://localhost:8094/origin-login.html'
  $F = WaitWindow 'MozillaWindowClass'; Start-Sleep 6; Place $F; Start-Sleep 1; Place $F
  foreach ($n in 2, 3) {
    Keys '^l' 300; Keys "http://localhost:8094/origin-login.html?n=$n" 100; Keys '{ENTER}' 3000
    Click 600 450 300; Keys '{TAB}' 200; Keys "ff-user$n" 200; Keys '{TAB}' 200; Keys "Fx-real-pass-$n" 200; Keys '{ENTER}' 3000
    Shot "$P-1-firefox-save-prompt-$n"
    Click 508 260 1500
  }
  Keys '^+q' 3000
  Get-Process firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  $nFirefox = Entries $ffProfile.FullName
  Log "[$P] Firefox profile $($ffProfile.Name): $nFirefox login(s) in logins.json"
  # ---- The two-key profile.
  Remove-Item -Recurse -Force $twoDir -ErrorAction SilentlyContinue
  Copy-Item -Recurse "$K\fixtures\firefox-twokeys" $twoDir
  Copy-Item "$K\fixtures\places.sqlite" "$twoDir\places.sqlite"
  Add-Content "$ffRoot\profiles.ini" "`r`n[Profile72]`r`nName=twokeys`r`nIsRelative=1`r`nPath=Profiles/krunk072.twokeys`r`n"
  $nTwo = Entries $twoDir
  Log "[$P] two-key profile: $nTwo login(s) in logins.json"

  # ---- Krunk on a fresh data folder.
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  if (Test-Path $aside) { Remove-Item -Recurse -Force $real -ErrorAction SilentlyContinue } elseif (Test-Path $real) { Rename-Item $real $aside }
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  $A = WaitWindow 'KrunkWindow'; Start-Sleep 6; Place $A
  function ToImports { Click 700 150 300; Keys '{END}' 1200 }
  SafeGo 'krunk://passwords' 3500
  Click 640 371 300; Keys $master 200
  Click 640 438 300; Keys $master 200
  Click 456 551 4000
  SafeGo 'krunk://settings#passwords' 4000
  ToImports
  Shot "$P-2-profiles"
  # With four profiles listed, after End: Import of Firefox default-release at y 261, the
  # two-key profile at 365 (Chrome 469, Edge 573); once a result line shows, all 27 px higher.
  Click 1071 261 7000
  Shot "$P-3-first-firefox-imported"
  ToImports; Click 1071 338 7000
  Shot "$P-4-twokeys-imported"
  ToImports; Click 1071 234 7000
  Shot "$P-5-again-nothing-new"
  SafeGo 'krunk://passwords' 3500
  Shot "$P-6-passwords"
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 3
  Copy-Item "$real\krunk.log" "$Shots\$P-krunk.log" -Force -ErrorAction SilentlyContinue
  Get-Content "$real\krunk.log" | Select-String 'import' | ForEach-Object { Log "[log] $($_.Line)" }
  Log "[$P] expected: Firefox $nFirefox, two-key $nTwo"
} finally {
  Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Remove-Item -Recurse -Force $real -ErrorAction SilentlyContinue
  if (Test-Path $aside) { Rename-Item $aside $real }
  foreach ($f in 'logins.json', 'key4.db', 'logins-backup.json') { if (Test-Path "$bak\$f") { Copy-Item "$bak\$f" "$($ffProfile.FullName)\$f" -Force } }
  Copy-Item "$bak\profiles.ini" "$ffRoot\profiles.ini" -Force
  Remove-Item -Recurse -Force $twoDir -ErrorAction SilentlyContinue
  Log "[$P] Firefox profile restored: $(Entries $ffProfile.FullName) login(s)"
}
