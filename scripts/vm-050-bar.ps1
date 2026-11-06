# Krunk 0.5.0 milestone 3: the bookmarks bar's folder dropdown gets the item menu, items can
# be dragged out of it onto the bar, and the page can be dragged from the address bar onto the
# bar or into a folder. Fresh profile (-Phase main), then -Phase restart checks the order kept.
# Screenshots "bar-<phase>-*"; the database is copied to p050b-<phase>.db at the end.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "bar-$Phase"
$BarMenu = @{ NewFolder = 51 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p050b'
if ($Phase -eq 'main') { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$barY = Y 90
$toolbarY = Y 56
$iconX = X 133          # the site icon in the address bar (CSS px of the shell)
$toolsX = X 43          # the "Tools" folder, first on the bar (about 70 DIP wide)
$rowY = Y 126           # the first row of the open folder dropdown
$rowX = X 48

if ($Phase -eq 'main') {
  # The folder first, then two bookmarks: [Tools, Example Domain (.com), Example Domain (.org)].
  # (On a page first: the new-tab page's address-bar focus would close the bar menu.)
  SafeGo 'https://example.com/' 3000
  RightClick (X 900) $barY 1500
  Shot "$P-0-bar-menu"
  Click (X 960) ($barY + [int]($BarMenu.NewFolder * $c.Scale)) 1500
  Keys 'Tools' 300; Keys '{ENTER}' 1500
  SafeKeys '^d' 1000
  SafeGo 'https://example.org/' 2500; SafeKeys '^d' 1000
  Shot "$P-1-folder-and-two"
  # example.com into Tools (the second item, centre about 8 + 70 + 2 + 66 DIP).
  Drag (X 146) $barY $toolsX $barY 20
  Start-Sleep 1
  Shot "$P-2-com-in-folder"
  # (R1) The item menu inside the open dropdown; Esc goes back to the dropdown.
  Click $toolsX $barY 1500
  Shot "$P-3-dropdown"
  RightClick $rowX $rowY 1200
  Shot "$P-4-item-menu-in-dropdown"
  Keys '{ESC}' 1000
  Shot "$P-5-back-to-dropdown"
  Keys '{ESC}' 600
  # (R3) The item dragged out of the dropdown onto the bar, after example.org.
  Click $toolsX $barY 1500
  Drag $rowX $rowY (X 600) $barY 25
  Start-Sleep 1
  Shot "$P-6-dragged-out-to-bar-end"
  # (R2) The page dragged from the site icon into Tools (rust-lang.org), then another page
  # to the front of the bar (iana.org).
  SafeGo 'https://www.rust-lang.org/' 4000
  Drag $iconX $toolbarY $toolsX $barY 25
  Start-Sleep 1
  Shot "$P-7-page-dragged-into-folder"
  SafeGo 'https://www.iana.org/' 4000
  Drag $iconX $toolbarY (X 12) $barY 25
  Start-Sleep 1
  Shot "$P-8-page-dragged-to-front"
  # Tools is the second item now (after the 180 DIP wide IANA item).
  Click (X 225) $barY 1500
  Shot "$P-9-folder-holds-rust"
  Keys '{ESC}' 600
} else {
  Shot "$P-1-restored-bar"
  Click (X 225) $barY 1500
  Shot "$P-2-restored-folder"
  Keys '{ESC}' 600
}
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[$P] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Copy-Item "$profile\Krunk\browser.db" "C:\pipeline\krunk\p050b-$Phase.db" -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'bookmark|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
