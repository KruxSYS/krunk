# Krunk 0.6.0 milestone 4 (R8–R11): the title-bar menu, the task manager (Shift+Esc; killing a
# renderer shows the crashed-tab page), the tab speaker icon with mute and "Mute site", and the
# new-tab settings (what opens, where, link tabs, new-tab search).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'tabs'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060t'
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
SafeGo 'https://example.com/' 3000
# Menu items are clicked relative to the popup window: 6 px padding, 30 px items, 11 px separators.
function PopupRectNow { $pw = [U]::FindWindow('KrunkPopup', [NullString]::Value); WindowRect $pw }
function MenuItem([int]$index, [int]$sepsBefore) { $pr = PopupRectNow; @([int]($pr.Left + 60 * $c.Scale), [int]($pr.Top + (6 + 30 * $index + 15 + 11 * $sepsBefore) * $c.Scale)) }
function ClickItem([int]$index, [int]$sepsBefore, [int]$wait = 1500) { $p = MenuItem $index $sepsBefore; Click $p[0] $p[1] $wait }
# Speaker/menu positions on the tab strip: tabs are 256 DIP wide from x = 8 (shrinking evenly).
function TabWidth([int]$n) { [Math]::Min(256, (($c.Right - $c.Left) / $c.Scale - 222) / [Math]::Max(1, $n)) }
function TabLeft([int]$i, [int]$n) { 8 + ((TabWidth $n) + 2) * $i }
# (R8) Right-click on the empty tab strip: Krunk's menu, not the system menu.
RightClick (X 700) (Y 15) 1200
Shot "$P-1-caption-menu"
$pr = PopupRectNow; Log "[$P] caption menu popup: $($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)"
Keys '{ESC}' 500
RightClick (X 700) (Y 15) 1200
ClickItem 0 0 1500            # New tab
Shot "$P-2-caption-new-tab"
SafeKeys '^w' 800
SafeKeys '^t' 500; SafeKeys 'https://example.org/' 100; SafeKeys '{ENTER}' 2500
RightClick (X 700) (Y 15) 1200
ClickItem 4 1 1500            # Bookmark all tabs
Shot "$P-3-bookmark-all-tabs"
Log "[$P] bookmark all: $(Get-Content "$profile\Krunk\krunk.log" | Select-String 'bookmarked' | Select -Last 1)"
RightClick (X 700) (Y 15) 1200
ClickItem 7 2 1500            # Minimize
$r = WindowRect $A; Log "[$P] after Minimize: window at $($r.Left),$($r.Top) (minimized windows sit at -32000)"
[U]::ShowWindow($A, 9) | Out-Null; Start-Sleep 1; Focus $A | Out-Null
Shot "$P-3b-restored"
# (R10) Audio: play with sound (tab 2 of 2), the speaker appears; click it to mute; "Mute site".
SafeGo 'http://localhost:8094/audio.html' 2500
Click (X 450) (Y 400) 400
SafeKeys 'p' 2500
Shot "$P-4-speaker"
$n = 2; $w = TabWidth $n; $spk = (TabLeft 1 $n) + $w - 46
Click (X $spk) (Y 18) 1500
Shot "$P-5-muted"
Log "[$P] muted: $(Get-Content "$profile\Krunk\krunk.log" | Select-String ': muted|: unmuted' | Select -Last 1)"
Click (X $spk) (Y 18) 1500
Shot "$P-6-unmuted"
Log "[$P] unmuted: $(Get-Content "$profile\Krunk\krunk.log" | Select-String ': muted|: unmuted' | Select -Last 1)"
RightClick (X ((TabLeft 1 $n) + 60)) (Y 18) 1200
Shot "$P-7-tab-menu-mute-site"
ClickItem 4 2 1500            # Mute site
Shot "$P-8-site-muted"
Log "[$P] site mute: $(Get-Content "$profile\Krunk\krunk.log" | Select-String 'site http' | Select -Last 1)"
RightClick (X ((TabLeft 1 $n) + 60)) (Y 18) 1200
Shot "$P-8b-tab-menu-unmute-site"
Keys '{ESC}' 500
Click (X 450) (Y 400) 300; SafeKeys 's' 500
# (R9) Shift+Esc: the task manager; end the example.com renderer (tab 1).
SafeKeys '+{ESC}' 4000
Shot "$P-9-task-manager"
Start-Sleep 2
Shot "$P-10-task-manager-2s"
# Sort by type so the tab rows are together, then click rows until the one named "Example Domain".
Click (X 500) (Y 232) 500            # the first row: "audio probe" (sorted by title)
Shot "$P-12-selected"
Click (X 1112) (Y 152) 3000          # End process
Shot "$P-13-after-end-process"
Log "[$P] task manager: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'task manager|renderer ended') -join ' ## ')"
SafeKeys '^2' 2000
Shot "$P-14-crashed-tab"
# (R11) New-tab settings: a new tab next to the current one, opening the homepage.
SafeGo 'krunk://settings' 3500
Shot "$P-15-settings-general"
Click (X 1020) (Y 411) 600; Keys 'The homepage' 300; Keys '{ENTER}' 1200
Click (X 1020) (Y 467) 600; Keys 'Next' 300; Keys '{ENTER}' 1200
Shot "$P-15b-new-tab-settings"
SafeKeys '^1' 1000
SafeKeys '^t' 3000
Shot "$P-16-new-tab-next-with-homepage"
Log "[$P] windows: $((Frames | ForEach-Object { [TW]::Txt($_) }) -join ' | ')"
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
