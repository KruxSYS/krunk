param([string]$Theme = 'dark', [string]$Label = 'after')
# Krunk 0.7.2 M5 (R6): no white while the window is resized. A black full-screen backdrop sits
# behind Krunk; tools\framecap.exe records the whole screen while the right edge, the bottom
# edge, the bottom-right corner and the left edge are dragged out and back (Krunk's own size
# loop), first on a page in the theme's colours (dark.html / light.html), then on the new tab
# page. scripts/frames/analyze.py white counts white frames on the build host (dark: anywhere;
# light: below the toolbar, whose own white is the theme). Also the 0.6.0 resize rows: Esc
# during a drag restores the size. Fresh profile (copy of sp-tpl) with the theme set.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "resize072-$Theme-$Label"
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p072r"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$profile\Krunk\browser.db"
@{ theme = $Theme } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
# The backdrop: a black borderless window over the whole screen, behind Krunk.
$bd = 'Add-Type -AssemblyName System.Windows.Forms; $f = New-Object Windows.Forms.Form; $f.BackColor = [Drawing.Color]::Black; $f.FormBorderStyle = 0; $f.ShowInTaskbar = $false; $f.StartPosition = 0; $f.Bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds; $f.Text = ''krunk-backdrop''; [Windows.Forms.Application]::Run($f)'
$backdrop = Start-Process powershell -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-EncodedCommand', ([Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($bd)))
Start-Sleep 2
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 800, 600, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
Log "[$P] version: $((Get-Item "$K\app\Krunk.dll").VersionInfo.ProductVersion) client top $($c.Top) scale $($c.Scale)"
# Pages whose scroll bars are in the theme's colours too (rdark: dark.html with color-scheme).
$page = if ($Theme -eq 'dark') { 'rdark' } else { 'light' }
SafeGo "http://localhost:8094/$page.html" 2500

function Press([int]$x, [int]$y) { [U]::SetCursorPos($x, $y) | Out-Null; AbsMove $x $y; Start-Sleep -Milliseconds 200; [SI]::Mouse(2, 0, 0) | Out-Null; Start-Sleep -Milliseconds 250 }
function Release { [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 400 }
# Out by $n steps of ($dx,$dy) every 20 ms, a pause, then back the same way.
function DragOutBack([int]$x, [int]$y, [int]$dx, [int]$dy, [int]$n) {
  Press $x $y
  for ($i = 1; $i -le $n; $i++) { AbsMove ($x + $dx * $i) ($y + $dy * $i); Start-Sleep -Milliseconds 20 }
  Start-Sleep -Milliseconds 300
  for ($i = $n - 1; $i -ge 0; $i--) { AbsMove ($x + $dx * $i) ($y + $dy * $i); Start-Sleep -Milliseconds 20 }
  Release
}
function Drags([string]$what) {
  $r = WindowRect $A
  DragOutBack ($r.Right - 2) ([int](($r.Top + $r.Bottom) / 2)) 20 0 18
  $r = WindowRect $A
  DragOutBack ([int](($r.Left + $r.Right) / 2)) ($r.Bottom - 2) 0 12 12
  for ($k = 0; $k -lt 3; $k++) {
    $r = WindowRect $A
    DragOutBack ($r.Right - 2) ($r.Bottom - 2) 20 12 12
  }
  # The left edge (the window moves): from x 300 out to the left and back.
  [U]::MoveWindow($A, 300, 0, 800, 600, $true) | Out-Null; Start-Sleep -Milliseconds 300
  $r = WindowRect $A
  DragOutBack ($r.Left + 2) ([int](($r.Top + $r.Bottom) / 2)) -20 0 12
  [U]::MoveWindow($A, 0, 0, 800, 600, $true) | Out-Null; Start-Sleep -Milliseconds 300
  $w = WindowRect $A
  Log "[$P] $what drags done, window $($w.Left),$($w.Top)-$($w.Right),$($w.Bottom)"
}
# Content starts below the toolbar and the bookmarks bar (light: white above is the theme's).
$ymin = [int]($c.Top + 112 * $c.Scale)
Log "[$P] ymin $ymin"
$cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', '0,0,1280,800', '--step', '4', '--seconds', '12', '--out', "$Shots\$P-page" -WindowStyle Hidden -PassThru
MoveTo 900 700; Start-Sleep -Milliseconds 700
Drags 'page'
$cap.WaitForExit(20000) | Out-Null
Log "[$P] capture page: $(Get-Content "$Shots\$P-page.json")"
Shot "$P-1-page"
SafeKeys '^t' 2500
$cap = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', '0,0,1280,800', '--step', '4', '--seconds', '12', '--out', "$Shots\$P-ntp" -WindowStyle Hidden -PassThru
MoveTo 900 700; Start-Sleep -Milliseconds 700
Drags 'ntp'
$cap.WaitForExit(20000) | Out-Null
Log "[$P] capture ntp: $(Get-Content "$Shots\$P-ntp.json")"
Shot "$P-2-ntp"
# (0.6.0 R5) Esc during a left-edge drag restores the size.
$r = WindowRect $A; $x1 = $r.Left + 2; $y1 = [int](($r.Top + $r.Bottom) / 2)
[U]::MoveWindow($A, 200, 0, 800, 600, $true) | Out-Null; Start-Sleep -Milliseconds 400
$r = WindowRect $A; $x1 = $r.Left + 2
Press $x1 $y1
for ($i = 1; $i -le 4; $i++) { AbsMove ($x1 + 40 * $i) $y1; Start-Sleep -Milliseconds 120 }
$w = WindowRect $A; Log "[$P] left dragged: $($w.Left),$($w.Top)-$($w.Right),$($w.Bottom)"
Keys '{ESC}' 400; $w = WindowRect $A; Log "[$P] after Esc: $($w.Left),$($w.Top)-$($w.Right),$($w.Bottom) (start 200,0-1000,600)"
Release
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Stop-Process -Id $backdrop.Id -Force -ErrorAction SilentlyContinue
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
