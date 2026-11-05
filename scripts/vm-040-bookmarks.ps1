# Krunk 0.4.0 milestone 4: the bookmarks bar's context menu, editor, drag into a folder and
# visibility modes. Fresh profile (or -Phase restart: the "hidden" mode survived). Screenshots
# "bm-*"; the bookmarks and settings at the end are copied to p040b-<phase>.db.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "bm-$Phase"
# Menus open at the click; item centres (CSS px below the click) for each menu.
$BarMenu = @{ AddPage = 21; NewFolder = 51; Always = 92; NewTabOnly = 122; Hide = 152 }
$ItemMenu = @{ OpenNewTab = 21; OpenNewWindow = 51; OpenPrivate = 81; Edit = 122; Delete = 152; AddPage = 193; NewFolder = 223; Always = 264; NewTabOnly = 294; Hide = 324 }
function MenuAt([int]$x, [int]$y, [int]$dy, [string]$name) {
  RightClick $x $y 1000
  Shot "$P-menu-$name"
  Click ($x + 60) ($y + $dy) 1200
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p040b'
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
$barY = [int]($c.Top + 90 * $c.Scale)
# Bar items "Example Domain" are about 133 DIP wide, from x = 8 DIP, 2 DIP apart.
function ItemX([int]$i) { [int]($c.Left + (8 + 135 * $i + 66) * $c.Scale) }
$emptyX = [int]($c.Left + 900 * $c.Scale)

if ($Phase -eq 'main') {
  SafeGo 'https://example.com/' 2500
  SafeKeys '^d' 1000
  SafeGo 'https://example.org/' 2500
  SafeKeys '^d' 1000
  Shot "$P-1-two-bookmarks"
  # (1) Empty bar space -> New folder... -> "Tools".
  MenuAt $emptyX $barY $BarMenu.NewFolder 'bar'
  Shot "$P-2-editor-new-folder"
  Keys 'Tools' 300; Keys '{ENTER}' 1500
  Shot "$P-3-folder-made"
  # (2) Drag the first bookmark onto the middle of the folder (third item).
  $from = ItemX 0; $to = [int]($c.Left + (8 + 135 * 2 + 30) * $c.Scale)
  Drag $from $barY $to $barY 20
  Start-Sleep 1
  Shot "$P-4-dragged-into-folder"
  # The folder (now the second item) holds it.
  $folderX = [int]($c.Left + (8 + 135 * 1 + 30) * $c.Scale)
  Click $folderX $barY 1500
  Shot "$P-5-folder-open"
  Keys '{ESC}' 500
  # (3) Edit the remaining bookmark: name "Org".
  MenuAt (ItemX 0) $barY $ItemMenu.Edit 'item'
  Shot "$P-6-editor"
  Keys 'Org' 300; Keys '{ENTER}' 1500
  Shot "$P-7-renamed"
  # (4) Delete it ("Org" is narrow now: 20 DIP into the first item).
  MenuAt ([int]($c.Left + 28 * $c.Scale)) $barY $ItemMenu.Delete 'item2'
  Shot "$P-8-deleted"
  # (5) Only on new tabs: hidden on example.org, shown on a new tab.
  MenuAt $emptyX $barY $BarMenu.NewTabOnly 'bar2'
  Start-Sleep 1
  Shot "$P-9-newtab-mode-on-page"
  SafeKeys '^t' 2000
  Shot "$P-10-newtab-mode-on-new-tab"
  # (6) Hide it (from the new tab's bar).
  MenuAt $emptyX $barY $BarMenu.Hide 'bar3'
  Start-Sleep 1
  Shot "$P-11-hidden"
} else {
  Shot "$P-restored"
  # Ctrl+Shift+B brings the hidden bar back.
  SafeKeys '^+b' 1500
  Shot "$P-ctrl-shift-b"
}
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[$P] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Copy-Item "$profile\Krunk\browser.db" "C:\pipeline\krunk\p040b-$Phase.db" -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'bookmark|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
