# Krunk 0.7.0 M2 (R4-R6): the address bar selects text on a drag (unfocused and focused), a
# double click selects a word and a triple click all; a click into the page blurs it (selection
# gone, formatted address back, keys go to the page); a new tab (Ctrl+T, the + button, the menu)
# puts the caret in the address bar, not in the new tab page's search box. Fresh profile.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'address'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p070a"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
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
SafeGo 'http://localhost:8094/login.html' 3000
SafeGo 'https://example.com/some/long/path?query=1' 4000
Click (X 640) (Y 400) 600
Shot "$P-1-unfocused-formatted"

# (R4) Drag in the unfocused bar: a selection, not a drag of the address.
Drag (X 200) (Y 56) (X 290) (Y 56) 12
Start-Sleep -Milliseconds 400
Shot "$P-2-drag-unfocused"
Log "[$P] drag in the unfocused bar selected: '$(Clip)'"
# Drag again in the focused bar, starting inside the selection.
SafeKeys '^a' 200
Drag (X 230) (Y 56) (X 330) (Y 56) 12
Start-Sleep -Milliseconds 400
Shot "$P-3-drag-focused"
Log "[$P] drag in the focused bar (from inside the selection) selected: '$(Clip)'"
DoubleClick (X 255) (Y 56); Start-Sleep -Milliseconds 400
Log "[$P] double click selected: '$(Clip)'"
# Three presses within the double-click time.
MoveTo (X 255) (Y 56)
for ($i = 0; $i -lt 3; $i++) { [U]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [U]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80 }
Start-Sleep -Milliseconds 400
Log "[$P] triple click selected: '$(Clip)'"
Shot "$P-4-triple"

# (R5) A click into the page: the bar gives up focus and selection, keys go to the page.
Click (X 640) (Y 420) 600
Shot "$P-5-page-click"
SafeKeys '{END}' 300
Log "[$P] after the page click the clipboard copy is: '$(Clip)' (nothing selected in the bar)"

# (R6) New tabs: the caret is in the address bar.
SafeKeys '^t' 1500
Keys 'ctrl-t-text' 300
Shot "$P-6-new-tab-ctrl-t"
SafeKeys '^a' 200
Log "[$P] Ctrl+T, typed: address bar holds '$(Clip)'"
SafeKeys '{ESC}' 200
# The + button sits right of the last tab (two tabs of 256 DIP from x = 8, 2 DIP apart).
Click (X (8 + 258 * 2 + 14)) (Y 18) 1500
Keys 'plus-text' 300
Shot "$P-7-new-tab-plus"
SafeKeys '^a' 200
Log "[$P] + button, typed: address bar holds '$(Clip)'"
SafeKeys '{ESC}' 200
Click ($c.Right - 20) (Y 56) 900
Click ($c.Right - 160) (Y 92) 1500
Keys 'menu-text' 300
Shot "$P-8-new-tab-menu"
SafeKeys '^a' 200
Log "[$P] menu New tab, typed: address bar holds '$(Clip)'"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
