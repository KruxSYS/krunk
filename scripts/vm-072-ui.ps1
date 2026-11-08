# Krunk 0.7.2 (R4, R5, R8-R11, R13): screenshots and checks of the reworked UI on the VM, and the
# 050-bar rows (bar, folder dropdown, drag) it touches. Fresh profile (copy of sp-tpl) with a seeded
# bookmarks bar: a folder "Tools" with a long title inside, 14 bookmarks (more than fit), and a
# bookmark with a 300-character address in "Other bookmarks".
#   light: the bar's overflow ("»" and its dropdown, after a narrower window), a reorder drag and a
#          drop on "»", the folder dropdown with the long title, the bookmark menu on a bar folder
#          and on an item inside the dropdown (the second popup window), the ⋮ menu, krunk://
#          bookmarks with the long address, then the Add login / Add card dialogs with an error
#   dark:  the two dialogs again
#   update: the dev-updates build (C:\pipeline\krunk\appdev: http://127.0.0.1:8096, test key) with
#          no server on 8096 (the check fails: a log line, no dot), then with a server offering
#          0.7.99 (the dot on ⋮ and the menu's first item).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'ui072'
$K = 'C:\pipeline\krunk'
$master = 'Krunk-test-master-1'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8096' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
$profile = "$K\p072u"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
$seed = @'
import sqlite3, sys
d = sqlite3.connect(sys.argv[1])
d.execute("DELETE FROM kv WHERE key='session'")
d.execute("DELETE FROM bookmarks WHERE id > 2")
cur = d.execute("INSERT INTO bookmarks(parent,is_folder,title,url,position,added) VALUES (1,1,'Tools',NULL,0,1)")
tools = cur.lastrowid
d.execute("INSERT INTO bookmarks(parent,is_folder,title,url,position,added) VALUES (?,0,?,?,0,1)", (tools, 'A very long bookmark title that would have wrapped onto two lines in the folder dropdown of 0.7.1', 'http://localhost:8094/links.html?long'))
d.execute("INSERT INTO bookmarks(parent,is_folder,title,url,position,added) VALUES (?,0,'Short one','http://localhost:8094/login.html',1,1)", (tools,))
for i in range(14):
    d.execute("INSERT INTO bookmarks(parent,is_folder,title,url,position,added) VALUES (1,0,?,?,?,1)", ('Bookmark number %d' % i, 'http://localhost:8094/links.html?b=%d' % i, i + 1))
long_url = 'http://localhost:8094/links.html?' + 'segment-' * 36 + 'end'
d.execute("INSERT INTO bookmarks(parent,is_folder,title,url,position,added) VALUES (2,0,?,?,0,1)", ('A bookmark with a long title and an address of three hundred characters, kept on one line', long_url[:300]))
d.commit()
print(len(long_url[:300]))
'@
Set-Content -Encoding ascii "$K\seed072.py" $seed
Log "[$P] seeded; long address length $(python "$K\seed072.py" "$profile\Krunk\browser.db")"
function SetTheme([string]$t) { @{ theme = $t } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"; Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")" }
function StartKrunk([string]$dir) {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$dir\Krunk.exe" -WorkingDirectory $dir
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
  $script:c = ClientRect $A
}
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function StopKrunk { foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1200 } }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }

# ---------------- light: bookmarks, menus, bookmarks page, dialogs
SetTheme 'light'
StartKrunk "$K\app"
SafeGo 'http://localhost:8094/links.html' 2500
Shot "$P-1-bar-overflow"
# "»": the last button of the bar (28 DIP wide, 8 DIP from the right).
Click ($c.Right - [int](22 * $c.Scale)) (Y 90) 1200
Shot "$P-2-overflow-open"
SafeKeys '{ESC}' 500
[U]::MoveWindow($A, 0, 0, 900, 760, $true) | Out-Null; Start-Sleep 1
Shot "$P-3-overflow-narrow"
$c = ClientRect $A
Click ($c.Right - [int](22 * $c.Scale)) (Y 90) 1200
Shot "$P-4-overflow-open-narrow"
SafeKeys '{ESC}' 500
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
$c = ClientRect $A
# A reorder: "Bookmark number 0" (the second item) dragged right by 300 DIP; then the same item
# dropped on "»" (to the end of the bar).
Drag (X 150) (Y 90) (X 450) (Y 90) 16
Shot "$P-5-after-reorder"
Drag (X 150) (Y 90) ($c.Right - [int](22 * $c.Scale)) (Y 90) 20
Shot "$P-6-after-drop-on-overflow"
Log "[$P] bar moves: $(KLog 'bookmark')"
# The folder dropdown (long title) and its tooltip.
Click (X 40) (Y 90) 1200
Shot "$P-7-folder-long-title"
MoveTo (X 150) (Y 120); Start-Sleep -Milliseconds 2000
Shot "$P-8-folder-tooltip"
# The bookmark menu on an item inside the dropdown: the second popup window over the first.
RightClick (X 150) (Y 120) 1200
Shot "$P-9-item-menu-in-dropdown"
SafeKeys '{ESC}' 400; SafeKeys '{ESC}' 400
# The bookmark menu on the bar folder, and the ⋮ menu to compare.
RightClick (X 40) (Y 90) 1200
Shot "$P-10-folder-menu-on-bar"
SafeKeys '{ESC}' 500
Click ($c.Right - 20) (Y 56) 1200
Shot "$P-11-main-menu"
SafeKeys '{ESC}' 500
# krunk://bookmarks: "Other bookmarks" (Tab past the search box, the three buttons and the
# "Bookmarks bar" and "Tools" folders).
SafeGo 'krunk://bookmarks' 3000
Click (X 1200) (Y 700) 300
foreach ($i in 1..7) { Keys '{TAB}' 120 }
Keys '{ENTER}' 1200
Shot "$P-12-bookmarks-long-address"
# Passwords & payments: a vault, then the two dialogs.
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
function Dialogs([string]$t) {
  Click (X 1200) (Y 700) 300
  Keys '{TAB}' 150; Keys '{TAB}' 150; Keys '{ENTER}' 1200
  Shot "$P-$t-1-add-login"
  Keys 'news.example.com' 200; Keys '{ENTER}' 800
  Shot "$P-$t-2-add-login-error"
  Keys '{ESC}' 600
  Click (X 1200) (Y 700) 300
  foreach ($i in 1..5) { Keys '{TAB}' 150 }
  Keys '{ENTER}' 1200
  Shot "$P-$t-3-add-card"
  Keys '4111 1111 1111 1111' 200; Keys '{TAB}' 150; Keys '13' 200; Keys '{ENTER}' 800
  Shot "$P-$t-4-add-card-error"
  Keys '{ESC}' 600
}
Dialogs 'light'
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-light-krunk.log" -ErrorAction SilentlyContinue
StopKrunk

# ---------------- dark: the dialogs again
SetTheme 'dark'
StartKrunk "$K\app"
SafeGo 'krunk://passwords' 3500
Keys $master 200; Keys '{ENTER}' 2500
Shot "$P-dark-0-unlocked"
Dialogs 'dark'
StopKrunk

# ---------------- the update notice (dev-updates build)
if (Test-Path "$K\appdev\Krunk.exe") {
  StartKrunk "$K\appdev"
  Start-Sleep 9
  Shot "$P-u1-no-server"
  Log "[$P] no server: $(KLog 'update check')"
  StopKrunk
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8096/'
  Start-Sleep 2
  StartKrunk "$K\appdev"
  Start-Sleep 9
  Shot "$P-u2-dot"
  Click ($c.Right - 20) (Y 56) 1200
  Shot "$P-u3-menu-update-item"
  Log "[$P] with the server: $(KLog 'update check')"
  StopKrunk
  Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8096' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
}
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] warnings: $(KLog 'WARN|ERROR')"
Log "[$P] done"
