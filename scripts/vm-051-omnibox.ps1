# Krunk 0.5.1: suggestions under the address bar while typing. A fresh profile visits a few
# pages and bookmarks one; then typing "exa" shows the dropdown (search row, the bookmark and
# the history pages), Down selects a row (the bar shows its address) while the field keeps the
# focus, Enter goes there; Esc closes the dropdown and keeps the typed text; a second Esc
# leaves the field. Screenshots "ob-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'ob'
foreach ($port in 8094) {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',"http://*:$port/"
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p051ob'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
function Title { [TW]::Txt($A) }
# (1) Some history and a bookmark.
SafeGo 'https://example.com/' 4000
SafeKeys '^d' 1200; Keys '{ENTER}' 1000
SafeGo 'http://localhost:8094/links.html' 3000
SafeGo 'http://localhost:8094/origin-login.html' 3000
SafeGo 'https://example.com/' 3000
# (2) Type: the dropdown.
SafeKeys '^l' 500
Keys 'exa' 1200
Shot "$P-1-typed-exa-dropdown"
Keys '{DOWN}' 800
Shot "$P-2-down-selects-first-row"
Keys '{DOWN}' 800
Shot "$P-3-down-selects-second-row"
Keys 'm' 600
Shot "$P-4-typing-continues-after-selection"
Keys '{ESC}' 800
Shot "$P-5-esc-closes-keeps-text"
Keys '{ESC}' 800
Shot "$P-6-second-esc-leaves-field"
# (3) Pick a row with Enter.
SafeKeys '^l' 500
Keys 'lin' 1200
Shot "$P-7-typed-lin"
Keys '{DOWN}' 600
Keys '{DOWN}' 600
Shot "$P-8-history-row-selected"
Keys '{ENTER}' 3000
Log "[pick] title after Enter: $(Title)"
Shot "$P-9-after-enter"
# (4) A click on a row.
SafeKeys '^l' 500
Keys 'orig' 1200
Shot "$P-10-typed-orig"
Click 400 118 3000
Log "[click] title after the click: $(Title)"
Shot "$P-11-after-click"
# (5) Search row: Enter without a selection searches as before.
SafeKeys '^l' 500
Keys 'zzqq' 1000
Shot "$P-12-search-only"
Keys '{ESC}' 500; Keys '{ESC}' 500
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'suggestion|address|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line)" }
