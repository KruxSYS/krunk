# Krunk 0.8.0 (B, items 8-14): the back/forward history menus. Five pages, right-click Back
# (newest first) and Forward (in order), a jump 3 back with the mouse and one with the keyboard,
# the button states after each jump (title and both menus), a disabled button showing nothing,
# dark at 125 % DPI, and a private window whose pages must not reach browser.db.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'nav080'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class PWS { [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i); public static int Style(IntPtr h) { return GetWindowLong(h, -16); } }
'@
$K = 'C:\pipeline\krunk'
$App = "$K\app"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p080n"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
function SetKrunk([string]$json) { Set-Content -Encoding ascii "$K\patch.json" $json; Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")" }
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
  $script:c = ClientRect $A
}
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function Title { [TW]::Txt((MainWindow)) }
function NavPopup {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  foreach ($h in [TW]::All()) {
    if ($pids -contains [TW]::Pid($h) -and [TW]::IsWindowVisible($h) -and [TW]::Cls($h) -eq 'KrunkPopup') { return (WindowRect $h) }
  }
  $null
}
function Pop([string]$what) { $r = NavPopup; if ($r) { "$what popup $($r.Left),$($r.Top)-$($r.Right),$($r.Bottom) ($($r.Right - $r.Left)x$($r.Bottom - $r.Top))" } else { "$what popup: none" } }
# The back and forward buttons (DIP, left end of the toolbar).
function BackXY { @((X 22), (Y 58)) }
function FwdXY { @((X 56), (Y 58)) }
function ItemXY([int]$i) { $r = NavPopup; @([int]($r.Left + 120 * $c.Scale), [int]($r.Top + (7 + 30 * $i + 15) * $c.Scale)) }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }

SetKrunk '{"theme":"light"}'
StartKrunk
foreach ($n in 1..5) { SafeGo "http://localhost:8094/nav.html?p=$n" 1500 }
Log "[$P] at '$(Title)'"
$b = BackXY; RightClick $b[0] $b[1] 1200
Log "[$P] back menu: $(Pop 'back')"
Shot "$P-1-back-menu-light"
# Item 3 (P2: three back from P5).
$it = ItemXY 2; Click $it[0] $it[1] 1800
Log "[$P] after a click on the third entry: '$(Title)' $(Pop 'after')"
$b = BackXY; RightClick $b[0] $b[1] 1200
Log "[$P] back menu at P2: $(Pop 'back')"
Shot "$P-2-back-menu-at-P2"
Keys '{ESC}' 500
$f = FwdXY; RightClick $f[0] $f[1] 1200
Log "[$P] forward menu at P2: $(Pop 'forward')"
Shot "$P-3-forward-menu-at-P2"
# Keyboard: Down once (P4), Enter.
Keys '{DOWN}' 300; Keys '{ENTER}' 1800
Log "[$P] keyboard jump: '$(Title)' $(Pop 'after')"
Shot "$P-4-after-keyboard-jump"
# Esc closes without a jump.
$b = BackXY; RightClick $b[0] $b[1] 1000; Keys '{ESC}' 600
Log "[$P] after Esc: '$(Title)' $(Pop 'esc')"
# Show full history (the last item: End, Enter).
$b = BackXY; RightClick $b[0] $b[1] 1000; Keys '{END}' 300; Keys '{ENTER}' 1500
Log "[$P] show full history: '$(Title)'"
Shot "$P-5-history-page"
# The page stays still under the history menu (A, item 6), and under a datalist (A).
SafeGo 'http://localhost:8094/select.html' 2000
$b = BackXY; RightClick $b[0] $b[1] 1000
Wheel (X 1000) (Y 600) 3
Log "[$P] wheel under the back menu: '$(Title)' $(Pop 'back')"
Keys '{ESC}' 500
Click (X 670) (Y (104 + 80)) 400; SafeKeys 'a' 900
$dl = @([TW]::All() | Where-Object { [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -like 'Chrome_WidgetWin_*' -and (([PWS]::Style($_)) -band 0x80000000) })
Wheel (X 1000) (Y 600) 3
Log "[$P] datalist open ($($dl.Count) popup), wheel outside: '$(Title)'"
Shot "$P-5b-datalist-wheel"
Keys '{ESC}' 400
# A new tab: both buttons disabled, no menu.
SafeKeys '^t' 1200
$b = BackXY; RightClick $b[0] $b[1] 900
Log "[$P] new tab, right-click Back: $(Pop 'disabled')"
Shot "$P-6-disabled"
Keys '{ESC}' 300
# A private window: the same menus; its pages must not be stored.
SafeKeys '^+n' 2500
$PW = (Frames | Where-Object { [TW]::Txt($_) -match 'Private|New tab' } | Select-Object -First 1)
foreach ($n in 7..9) { SafeGo "http://localhost:8094/nav.html?p=$n" 1500 }
$b = BackXY
$pc = ClientRect ([U]::GetForegroundWindow()); $bx = [int]($pc.Left + 22 * $pc.Scale); $by = [int]($pc.Top + 58 * $pc.Scale)
RightClick $bx $by 1200
Log "[$P] private back menu: $(Pop 'private')"
Shot "$P-7-private-back-menu"
Keys '{DOWN}' 200; Keys '{ENTER}' 1800
Log "[$P] private jump: '$([TW]::Txt([U]::GetForegroundWindow()))'"
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$q = @'
import sqlite3, sys
d = sqlite3.connect(sys.argv[1])
urls = [r[0] for r in d.execute("SELECT url FROM urls")]
kv = " ".join(str(r[0]) for r in d.execute("SELECT value FROM kv"))
priv = lambda u: any(x in u for x in ('nav.html?p=7', 'nav.html?p=8', 'nav.html?p=9'))
print('urls', len(urls), 'normal test pages', sum(1 for u in urls if 'nav.html?p=' in u and not priv(u)), 'private test pages in urls', sum(1 for u in urls if priv(u)), 'in kv', sum(1 for x in ('p=7', 'p=8', 'p=9') if 'nav.html?' + x in kv))
'@
Set-Content -Encoding ascii "$K\q080.py" $q
Log "[$P] browser.db: $(python "$K\q080.py" "$profile\Krunk\browser.db")"
Log "[$P] light WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk-light.log"

# Dark at 125 %.
powershell -NoProfile -File "$K\vm-dpi.ps1" -Percent 125 | Out-Null
Start-Sleep 3
SetKrunk '{"theme":"dark"}'
StartKrunk
foreach ($n in 1..3) { SafeGo "http://localhost:8094/nav.html?p=$n" 1500 }
$b = BackXY; RightClick $b[0] $b[1] 1200
Log "[$P] dark 125 % (scale $($c.Scale)): $(Pop 'back')"
Shot "$P-8-back-menu-dark-125"
$it = ItemXY 1; Click $it[0] $it[1] 1800
Log "[$P] dark 125 % jump 2 back: '$(Title)'"
if (Focus $A) { SafeKeys '%{F4}' 2000 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
powershell -NoProfile -File "$K\vm-dpi.ps1" -Percent 100 | Out-Null
Log "[$P] dark WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count)"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk-dark.log"
Log "[$P] done"
