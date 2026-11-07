# Krunk 0.6.0 milestone 13 (R24): Picture-in-Picture. On www/8094/pip.html (two muted videos)
# the toolbar shows the PiP button; Alt+P opens the chooser, a row sends the video to the
# engine's floating window (a separate always-on-top window on the desktop), Alt+P takes it
# back; with one video (#one) Alt+P goes straight in; the floating window's "Back to tab"
# activates the tab from another tab, its close button does not; the automatic setting puts a
# playing video in the window on a tab switch and takes it back on return.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "pip-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060p'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1000, 700, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
# The engine's floating video window: a visible top-level window of a Krunk process that is
# neither a Krunk frame nor a popup (the spike: class Chrome_WidgetWin_1, titled by the site).
function PipWindow {
  $pids = KrunkPids
  [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -notlike 'Krunk*' -and [TW]::Txt($_) -ne '' } | Select-Object -First 1
}
function PipLog([string]$what) {
  $h = PipWindow
  if ($h) { $r = WindowRect $h; Log "[$P] ${what}: floating window '$([TW]::Txt($h))' ($([TW]::Cls($h))) $($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { Log "[$P] ${what}: no floating window" }
  $h
}
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
# (1) Two videos: the button; Alt+P opens the chooser; the first row.
SafeGo 'http://localhost:8094/pip.html' 4000
Shot "$P-1-button"
SafeKeys '%p' 1500
$pr = PopupRectNow; Log "[$P] chooser popup: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"
Shot "$P-2-chooser"
if ($pr) { Click ($pr.Left + 120) ($pr.Top + 52) 2500 }
PipLog 'after the pick' | Out-Null
Shot "$P-3-pip-window"
# (2) Alt+P again takes it back.
Focus $A | Out-Null
SafeKeys '%p' 2000
PipLog 'after Alt+P' | Out-Null
Shot "$P-4-left"
# (3) One video: straight in; "Back to tab" from another tab; the close button pauses only.
SafeGo 'http://localhost:8094/pip.html?one#one' 4000
SafeKeys '%p' 2500
$h = PipLog 'one video'
Shot "$P-5-one-video-pip"
SafeKeys '^t' 2000
Shot "$P-6-other-tab"
if ($h) {
  $r = WindowRect $h
  MoveTo ([int](($r.Left + $r.Right) / 2)) ([int](($r.Top + $r.Bottom) / 2)); Start-Sleep -Milliseconds 800
  Shot "$P-7-pip-hover"
  Click ($r.Right - 40) ($r.Top + 16) 2500                                    # back to tab (top-right row: minimize, back to tab, close)
}
Shot "$P-8-back-to-tab"
Log "[$P] foreground after back to tab: $([TW]::Cls([U]::GetForegroundWindow())) '$([TW]::Txt([U]::GetForegroundWindow()))'"
Focus $A | Out-Null
SafeKeys '%p' 2500                                                              # in again
$h = PipLog 'in again'
SafeKeys '^t' 2000
if ($h) {
  $r = WindowRect $h
  MoveTo ([int](($r.Left + $r.Right) / 2)) ([int](($r.Top + $r.Bottom) / 2)); Start-Sleep -Milliseconds 800
  Click ($r.Right - 17) ($r.Top + 16) 2500                                    # close (the rightmost control)
}
PipLog 'after close' | Out-Null
Shot "$P-9-closed-stays-on-other-tab"
Log "[$P] foreground after close: $([TW]::Cls([U]::GetForegroundWindow())) '$([TW]::Txt([U]::GetForegroundWindow()))'"
Focus $A | Out-Null
SafeKeys '^w' 1500
# (4) Automatic Picture-in-Picture: the setting (the settings-page coordinates need the
# 1280x760 window of the other scripts), a tab switch away and back.
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
$c = ClientRect $A
SafeGo 'krunk://gpu' 1500
SafeGo 'krunk://settings' 3000
Click (X 968) (Y 146) 300; Keys 'Automatic Picture' 1200
Shot "$P-10-setting"
Click (X 1098) (Y 294) 1500                                                   # the row's checkbox (two-line description)
Shot "$P-11-setting-on"
SafeGo 'http://localhost:8094/pip.html?one#one' 4000
SafeKeys '^t' 2500
PipLog 'auto after the switch' | Out-Null
Shot "$P-12-auto-pip"
SafeKeys '^2' 2500                                                              # back to the video's tab (tab 2)
PipLog 'auto after the return' | Out-Null
Shot "$P-13-auto-back"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'picture|back to tab') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
