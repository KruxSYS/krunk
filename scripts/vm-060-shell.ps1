# Krunk 0.6.0 milestone 1 (R1–R4): the ⋮ menu and tab menu fit their content (no gap), a
# right-click inside an open bookmark-folder dropdown shows the item menu over the still-open
# dropdown (Esc returns to it, actions work on the entry), the zoom chip follows Ctrl+wheel at
# once, and the address-bar placeholder follows the search engine in every window.
#   -Theme light|dark (Windows apps mode, restored afterwards)   -Dpi 100|125
param([string]$Theme = 'light', [int]$Dpi = 100)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "shell-$Theme-$Dpi"
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$lightBefore = (Get-ItemProperty $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $(if ($Theme -eq 'dark') { 0 } else { 1 }) -Type DWord
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
& powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent $Dpi | ForEach-Object { Log "[$P] dpi: $_" }
Start-Sleep 2
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "C:\pipeline\krunk\p060s-$Theme-$Dpi"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
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
$W = [int](($c.Right - $c.Left) / $c.Scale)   # shell width in DIP
$barY = Y 90; $toolbarY = Y 56; $menuX = X ($W - 20); $toolsX = X 43; $rowY = Y 126; $rowX = X 48
Log "[$P] client $($c.Left),$($c.Top)-$($c.Right),$($c.Bottom) scale $($c.Scale) width $W DIP"

# (R1) The ⋮ menu: its window ends right after "Exit".
SafeGo 'https://example.com/' 3000
Click $menuX $toolbarY 1200
Shot "$P-1-menu"
$pw = [U]::FindWindow('KrunkPopup', [NullString]::Value)
$pr = WindowRect $pw
Log "[$P] menu popup window: $($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom) = $($pr.Right - $pr.Left)x$($pr.Bottom - $pr.Top) px"
Keys '{ESC}' 600
# The toggle fix of 0.5.0 still holds: three clicks = open, close, open.
Click $menuX $toolbarY 700; Click $menuX $toolbarY 700; Click $menuX $toolbarY 900
Shot "$P-2-menu-after-3-clicks"
Keys '{ESC}' 600
# The tab menu fits too.
RightClick (X 100) (Y 15) 1000
Shot "$P-3-tab-menu"
Keys '{ESC}' 600

# (R2) A folder with two bookmarks on the bar; right-click inside the open dropdown.
RightClick (X 900) $barY 1500
Click (X 960) ($barY + [int](51 * $c.Scale)) 1500
Keys 'Tools' 300; Keys '{ENTER}' 1500
SafeKeys '^d' 1000
SafeGo 'https://example.org/' 2500; SafeKeys '^d' 1000
Drag (X 146) $barY $toolsX $barY 20
Start-Sleep 1
Drag (X 146) $barY $toolsX $barY 20
Start-Sleep 1
Shot "$P-4-two-in-folder"
Click $toolsX $barY 1500
Shot "$P-5-dropdown"
RightClick $rowX $rowY 1200
Shot "$P-6-item-menu-over-dropdown"
$pops = @(Get-Process Krunk | ForEach-Object { $_.Id })
Log "[$P] popup windows visible: $(([U]::FindWindow('KrunkPopup', [NullString]::Value) -ne [IntPtr]::Zero))"
Keys '{ESC}' 1000
Shot "$P-7-esc-back-to-dropdown"
Keys '{DOWN}' 300; Keys '{DOWN}' 300
Shot "$P-7b-dropdown-keyboard"
# The menu's action works on the entry: "Open in new tab" opens the first item (example.com).
RightClick $rowX $rowY 1200
Shot "$P-8-item-menu-again"
Click ($rowX + [int](60 * $c.Scale)) ($rowY + [int](20 * $c.Scale)) 2500
Shot "$P-9-after-open-in-new-tab"
Keys '{ESC}' 500
# Click away (on the page) closes both.
Click $toolsX $barY 1500
RightClick $rowX $rowY 1200
Click (X 640) (Y 400) 1000
Shot "$P-10-click-away-closes-both"

# (R3) Ctrl+wheel zooms the page: the chip shows the new zoom at once (shot ~150 ms later).
SafeGo 'https://example.com/' 3000
$cx = X 640; $cy = Y 400
[U]::SetCursorPos($cx, $cy) | Out-Null; AbsMove $cx $cy; Start-Sleep -Milliseconds 200
[U]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80
[SI]::Wheel(120) | Out-Null; Start-Sleep -Milliseconds 150
Shot "$P-11-ctrl-wheel-150ms"
[SI]::Wheel(120) | Out-Null; Start-Sleep -Milliseconds 150
Shot "$P-12-ctrl-wheel-twice-150ms"
[U]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 500
Shot "$P-13-after-wheel"
SafeKeys '^0' 300
Shot "$P-14-ctrl-0-chip-hidden"
SafeKeys '^{+}' 300
Shot "$P-15-ctrl-plus"
SafeKeys '^0' 500

# (R4) Two windows; the search engine changes in Settings of window A; window B's placeholder follows.
SafeKeys '^n' 2500
$B = (Frames | Where-Object { $_ -ne $A } | Select-Object -First 1)
if ($B) { [U]::MoveWindow($B, 300, 200, 900, 500, $true) | Out-Null; Start-Sleep 1 }
Shot "$P-16-window-b-placeholder-ddg"
Focus $A | Out-Null
SafeGo 'krunk://settings' 3500
Shot "$P-17-settings"
# The search-engine select (first row of General, right column); chosen by typing.
# The select sits in the settings page's centred column: its place depends on the scale.
$selX = if ($Dpi -eq 125) { 888 } else { 1047 }
Click (X $selX) (Y 300) 700
Keys 'Brave' 300; Keys '{ENTER}' 1500
Shot "$P-18-engine-brave"
Focus $B | Out-Null; Start-Sleep -Milliseconds 500
Shot "$P-19-window-b-placeholder-brave"
Focus $A | Out-Null
SafeKeys '^t' 1500
Shot "$P-20-new-tab-placeholder"

# Close every window with Alt+F4 (a normal exit: the log has no killed renderers).
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
if ($Dpi -ne 100) { & powershell -NoProfile -File C:\pipeline\krunk\vm-dpi.ps1 -Percent 100 | ForEach-Object { Log "[$P] dpi: $_" } }
if ($null -ne $lightBefore) { Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $lightBefore -Type DWord }
Log "[$P] done"
