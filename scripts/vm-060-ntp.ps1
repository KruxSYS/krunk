# Krunk 0.6.0 milestone 6 (R14–R16): the new tab page's shortcut-keys section, the customize
# panel (background colour and an image with dim/blur, grid, own list, add/edit/reorder tiles,
# show/hide sections) surviving a restart, and the appearance options (accent, colours, density,
# font size, square tabs, hidden toolbar items) applied live.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "ntp-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p060n'
if ($Phase -eq 'main') { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force $profile | Out-Null
# A background picture for the file dialog (a generated PNG in the test folder).
$pic = 'C:\pipeline\krunk\www\bg.png'
if (-not (Test-Path $pic)) {
  Add-Type -AssemblyName System.Drawing
  $bmp = New-Object System.Drawing.Bitmap 640, 400
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Point]::new(0, 0)), ([System.Drawing.Point]::new(640, 400)), ([System.Drawing.Color]::FromArgb(255, 30, 60, 140)), ([System.Drawing.Color]::FromArgb(255, 220, 90, 40))
  $g.FillRectangle($br, 0, 0, 640, 400)
  $bmp.Save($pic, [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
}
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
if ($Phase -eq 'restart') {
  Shot "$P-0-after-restart"
  SafeGo 'https://example.com/' 2500
  Shot "$P-1-appearance-after-restart"
  foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
  Start-Sleep 2
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
  Log "[$P] done"
  return
}
# Some history for the tiles.
SafeGo 'https://example.com/' 2500
SafeGo 'https://example.org/' 2500
SafeKeys '^t' 2500
Shot "$P-1-newtab-with-shortcuts"
# (R14) Hide the shortcut keys ("Hide" at the right of the section's title).
Click (X 1013) (Y 475) 1200
Shot "$P-2-shortcuts-hidden"
# (R15) Customize (the button at the bottom right). The dialog is centred: its controls sit at
# fixed places in the 1280x760 window (measured from the shots).
Click (X 1200) (Y 715) 1200
Shot "$P-3-panel"
Click (X 610) (Y 276) 300; Keys '^a' 100; Keys '#1e3a8a' 200; Keys '{TAB}' 1200
Shot "$P-4-color"
Click (X 624) (Y 328) 2500          # Choose image…
Shot "$P-5-file-dialog"
Keys $pic 300; Keys '{ENTER}' 3000
Shot "$P-6-image"
Click (X 640) (Y 376) 300; Keys '{END}' 800            # dim to the maximum
Click (X 640) (Y 404) 300; Keys '{RIGHT}{RIGHT}{RIGHT}{RIGHT}{RIGHT}' 800   # blur 5 px
Shot "$P-7-dim-blur"
Click (X 506) (Y 528) 800           # My own list only
Click (X 580) (Y 572) 200; Keys '^a' 100; Keys '4' 100; Keys '{TAB}' 800   # 4 columns
Shot "$P-8-own-list-4-columns"
Keys '{ESC}' 800
Shot "$P-8b-page-customized"
# Add two shortcuts by hand (the "+ Add shortcut" tile is the first slot of the empty own
# list), then drag the second before the first.
Click (X 318) (Y 350) 1200
Shot "$P-9-add-dialog"
Click (X 640) (Y 394) 300; Keys 'Krunk site' 200
Click (X 640) (Y 458) 300; Keys 'https://krunk.209.wf/' 200
Click (X 778) (Y 534) 1500
Shot "$P-10-added"
Click (X 456) (Y 350) 1200
Click (X 640) (Y 394) 300; Keys 'Example' 200
Click (X 640) (Y 458) 300; Keys 'https://example.org/' 200
Click (X 778) (Y 534) 1500
Shot "$P-10b-two-added"
Drag (X 456) (Y 330) (X 250) (Y 330) 25
Start-Sleep 1
Shot "$P-11-reordered"
# (R16) Appearance: the green accent swatch, compact density, 15 px font, square tabs, no zoom chip.
SafeGo 'krunk://settings#appearance' 3500
Shot "$P-12-appearance-settings"
Click (X 902) (Y 446) 1200           # green swatch
Shot "$P-12b-accent"
Click (X 1030) (Y 680) 500; Keys 'Compact' 200; Keys '{ENTER}' 1200
Click (X 1030) (Y 734) 500; Keys '15' 200; Keys '{ENTER}' 1200
Keys '{TAB}' 300; Keys ' ' 800        # Rounded tabs off
Keys '{TAB}' 300; Keys '{TAB}' 300; Keys ' ' 800   # Show the zoom indicator off
Shot "$P-13-appearance-applied"
SafeGo 'https://example.com/' 2500
SafeKeys '^{+}' 800
Shot "$P-14-page-with-appearance"
Log "[$P] windows: $((Frames | ForEach-Object { [TW]::Txt($_) }) -join ' | ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Copy-Item "$profile\Krunk\browser.db" "C:\pipeline\krunk\shots\$P-end.db" -ErrorAction SilentlyContinue
Log "[$P] done"
