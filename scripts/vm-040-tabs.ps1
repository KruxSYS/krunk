# Krunk 0.4.0 milestone 1: "Close tabs to the right" / "Close other tabs" (tab menu) with
# Ctrl+Shift+T bringing them back in order, and Ctrl+J / the downloads button switching to an
# open Downloads tab (also from another window). Fresh profile. Screenshots "tabs-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'tabs'
# The tab menu opens at the click; its items (CSS px below the click, 1x scale): Reload 21,
# Duplicate 51, Move to new window 81, New window 122, Close tab 163, Close other tabs 193,
# Close tabs to the right 223.
function TabMenu([IntPtr]$h, [int]$i, [int]$n, [int]$dy) {
  $t = TabPoint $h $i $n
  RightClick $t[0] $t[1] 1200
  Click ($t[0] + 60) ($t[1] + $dy) 2000
}
function Titles([string]$label) {
  Log "[$label] foreground: '$([TW]::Txt([U]::GetForegroundWindow()))'"
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p040t'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
SafeGo 'https://example.com/?1' 2000
foreach ($n in 2..5) { SafeKeys '^t' 1000; SafeGo "https://example.com/?$n" 1800 }
SafeKeys '^1' 1000
Shot "$P-1-five-tabs"

# Close tabs to the right of tab 2: tabs 3-5 go; tab 1 stays active.
TabMenu $A 1 5 223
Shot "$P-2-closed-right"
Titles 'closed-right'
# Ctrl+Shift+T three times: 3, 4, 5 come back in their places.
SafeKeys '^+t' 1500; SafeKeys '^+t' 1500; SafeKeys '^+t' 2000
Shot "$P-3-reopened"
# Close other tabs on tab 4 (active is tab 1, which closes too: tab 4 becomes active).
TabMenu $A 3 5 193
Shot "$P-4-closed-others"
Titles 'closed-others'

# Ctrl+J opens Downloads; a new tab, then Ctrl+J again switches back instead of opening another.
SafeKeys '^j' 2000
SafeKeys '^t' 1000; SafeGo 'https://example.org/' 2000
SafeKeys '^j' 2000
Shot "$P-5-ctrl-j-again"
Titles 'ctrl-j-again'
# From a second window: Ctrl+J brings the first window (with its Downloads tab) to the front.
SafeKeys '^n' 2500
$B = [U]::GetForegroundWindow()
[U]::MoveWindow($B, 300, 200, 900, 500, $true) | Out-Null; Start-Sleep 1
SafeKeys '^j' 2500
Shot "$P-6-ctrl-j-from-window-b"
Titles 'ctrl-j-from-b'
KrunkWindows 'ctrl-j-from-b'
# The toolbar's downloads button does the same (it shows once a download ran; not here).

foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'closing|tab |window|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
