# Krunk 0.7.2 (R1-R3): the address bar on a new tab and while typing. Merges the rows of
# 070-address (typing right after Ctrl+T, the + button, the menu), 060-address (copy of the
# unedited address, paste) and 051-omnibox (a suggestion picked with the keyboard).
# tools\framecap records the bar's text area (step 1, light theme): around each new tab (no frame
# may show text: the profile has an Accept-Language override, so new tabs start at about:blank,
# which 0.7.1 showed), and while 5 letters are typed along an inline completion (no frame without
# the selected completion once it has appeared). scripts/frames/analyze.py addr on the host.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'addr072'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p072a"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$profile\Krunk\browser.db"
@{ theme = 'light'; headers = @{ acceptLanguage = 'de-DE,de' } } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function Clip { Set-Clipboard -Value '(empty)'; SafeKeys '^c' 400; "$(Get-Clipboard)" }
# The bar's text area.
$rect = "$(X 150),$(Y 44),$([int](700 * $c.Scale)),$([int](24 * $c.Scale))"
function Capture([string]$name, [int]$seconds) {
  Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', '1', '--seconds', $seconds, '--out', "$Shots\$P-$name" -WindowStyle Hidden -PassThru
}
# History for the completion and the suggestions.
foreach ($u in 'http://localhost:8094/login.html', 'http://localhost:8094/links.html', 'http://localhost:8094/dark.html') { SafeGo $u 2500 }
Shot "$P-0-page"

# (R1, R2) New tabs: no text in the bar from the first frame on; typing lands in the bar.
$cap = Capture 'newtab-ctrl-t' 2; Start-Sleep -Milliseconds 500
SafeKeys '^t' 1200
$cap.WaitForExit(10000) | Out-Null
Keys 'ctrl-t-text' 300; Shot "$P-1-ctrl-t-typed"
SafeKeys '^a' 200; Log "[$P] Ctrl+T, typed: address bar holds '$(Clip)'"
SafeKeys '{ESC}' 200; SafeKeys '^w' 800
# The + button right of the one tab (256 DIP from x = 8, then 14).
$cap = Capture 'newtab-plus' 2; Start-Sleep -Milliseconds 500
Click (X (8 + 258 + 14)) (Y 18) 1200
$cap.WaitForExit(10000) | Out-Null
Keys 'plus-text' 300; Shot "$P-2-plus-typed"
SafeKeys '^a' 200; Log "[$P] + button, typed: address bar holds '$(Clip)'"
SafeKeys '{ESC}' 200; SafeKeys '^w' 800
Click ($c.Right - 20) (Y 56) 900
$cap = Capture 'newtab-menu' 2; Start-Sleep -Milliseconds 500
Click ($c.Right - 160) (Y 92) 1200
$cap.WaitForExit(10000) | Out-Null
Keys 'menu-text' 300; Shot "$P-3-menu-typed"
SafeKeys '^a' 200; Log "[$P] menu New tab, typed: address bar holds '$(Clip)'"
SafeKeys '{ESC}' 200

# (R3) Five letters along the inline completion ("local" -> localhost:8094/...).
SafeKeys '^l' 400
$cap = Capture 'typing' 3; Start-Sleep -Milliseconds 400
foreach ($ch in 'l', 'o', 'c', 'a', 'l') { Keys $ch 160 }
Start-Sleep -Milliseconds 600
Shot "$P-4-typed-local"
$cap.WaitForExit(10000) | Out-Null
Log "[$P] after typing 'local' the bar selects: '$(Clip)'"
# Backspace removes the completion; Esc twice gives the page's address back.
Keys '{BACKSPACE}' 400; Shot "$P-5-backspace"
SafeKeys '{ESC}' 300; SafeKeys '{ESC}' 300

# (060-address) Copy of the unedited address; paste and Enter.
SafeKeys '^l' 300; Keys '^c' 300
Log "[$P] clipboard after Ctrl+C on the address: '$(Get-Clipboard -Raw)'"
Set-Clipboard -Value 'http://localhost:8094/login.html'
SafeKeys '^l' 300; Keys '^v' 300; Keys '{ENTER}' 2500
Log "[$P] after paste and Enter title: $([TW]::Txt($A))"
# (051-omnibox) A suggestion picked with the arrow keys.
SafeKeys '^l' 300; Keys 'links' 900; Keys '{DOWN}' 400; Keys '{ENTER}' 2500
Log "[$P] after a suggestion pick title: $([TW]::Txt($A))"
Shot "$P-6-suggestion-picked"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
