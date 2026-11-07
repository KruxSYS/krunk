# Krunk 0.6.0 milestone 7 (R19, R18): permission prompts for camera, microphone, location,
# clipboard, MIDI, screen sharing and notifications (fake media devices through the engine
# flag in Settings → Advanced), the remembered answers in Settings → Permissions and in the
# lock popup, the camera/microphone chip, and the site data page (sort, select, clear).
#   -Phase flag     fresh profile: set the fake-device flag, then quit (needs a restart)
#   -Phase main     the flows
param([string]$Phase = 'flag')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "perms-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
foreach ($port in '8094', '8443') {
  $prefix = if ($port -eq '8443') { 'https://*:8443/' } else { 'http://*:8094/' }
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',$prefix
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p060p'
if ($Phase -eq 'flag') { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
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
# The visible popup window (the shell keeps a second, hidden popup for item menus).
function PopupRectNow {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
# Answers the open prompt: Allow is the right-most button, Block the one left of it.
function Answer([string]$which, [int]$wait = 1500) {
  $pr = PopupRectNow
  if (-not $pr) { Log "[$P] no prompt popup to answer ($which)"; return }
  Log "[$P] prompt popup: $($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom) -> $which"
  $dx = if ($which -eq 'allow') { 50 } else { 130 }
  Click ($pr.Right - $dx) ($pr.Bottom - 28) $wait
}
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1; Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue }
if ($Phase -eq 'flag') {
  SafeGo 'krunk://settings#advanced' 3500
  Shot "$P-0-advanced"
  # The extra-flags field: Advanced → "Extra Chromium flags" (measured at 100 %: x≈900, y≈668).
  Click (X 900) (Y 668) 500; Keys '--use-fake-device-for-media-stream' 300; Keys '{TAB}' 1500
  Shot "$P-1-flag-set"
  Quit
  $flags = (Get-ChildItem "$profile\Krunk" -Filter *.json -Recurse | Select-String 'use-fake-device' | Select-Object -First 1)
  Log "[$P] flag stored: $(if ($flags) { $flags.Line.Trim() } else { 'NOT FOUND' })"
  Log "[$P] done"
  return
}
SafeGo 'https://www.krunk-test.test:8443/perm.html' 3500
Click (X 450) (Y 400) 400
# Camera: prompt → Allow (remembered); the chip shows while the stream runs.
SafeKeys '1' 1500
Shot "$P-2-camera-prompt"
Answer 'allow'
Shot "$P-3-camera-allowed-chip"
Start-Sleep 4
# Microphone: Block.
Click (X 450) (Y 400) 300; SafeKeys '2' 1500
Shot "$P-4-mic-prompt"
Answer 'block'
Shot "$P-5-mic-blocked"
# Location: dismiss with Esc (asks again next time), then Block.
Click (X 450) (Y 400) 300; SafeKeys '3' 1500
Shot "$P-6-geo-prompt"
Keys '{ESC}' 1500
Click (X 450) (Y 400) 300; SafeKeys '3' 1500
Shot "$P-7-geo-asks-again"
Answer 'block'
# Notifications (through the shim): Allow.
Click (X 450) (Y 400) 300; SafeKeys '4' 1500
Shot "$P-8-notifications-prompt"
Answer 'allow' 2500
Shot "$P-9-notifications-allowed"
# Clipboard (Allow), MIDI (Block), screen share (Allow).
Click (X 450) (Y 400) 300; SafeKeys '5' 1500; Shot "$P-10-clipboard-prompt"; Answer 'allow' 1200
Click (X 450) (Y 400) 300; SafeKeys '5' 1500                       # again with the page focused: reads
Click (X 450) (Y 400) 300; SafeKeys '6' 1500; Shot "$P-11-midi-prompt"; Answer 'block' 1200
Click (X 450) (Y 400) 300; SafeKeys '7' 1500; Shot "$P-12-screen-prompt"; Answer 'allow' 2500
Shot "$P-13-page-results"
# Camera again: no prompt (remembered).
Click (X 450) (Y 400) 300; SafeKeys '1' 1500
Shot "$P-14-camera-remembered"
Start-Sleep 3
# The lock popup lists the site's permissions.
Click (X 133) (Y 56) 1500                                              # the lock (133 DIP)
$pr = PopupRectNow; Log "[$P] lock popup: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"
Shot "$P-15-lock-popup-permissions"
Keys '{ESC}' 500
# Settings → Permissions: defaults and the exceptions.
SafeGo 'krunk://settings#permissions' 3500
Shot "$P-16-settings-permissions"
Keys '{END}' 1200
Shot "$P-16b-settings-exceptions"
# (R18) Site data: the list with usage and last used, sort by usage, select, clear.
SafeGo 'https://example.com/' 2500
SafeGo 'krunk://storage' 4000
Shot "$P-17-site-data"
Click (X 700) (Y 146) 400; Keys '{TAB}' 300; Keys '{DOWN}' 1000      # the sort select: usage
Shot "$P-18-site-data-sorted"
Keys '{DOWN}' 1000                                                    # last used
Shot "$P-18b-site-data-last-used"
Keys '{DOWN}' 300; Keys '{UP}' 300                                    # back to usage (DOWN past the end stays)
Click (X 700) (Y 146) 400; Keys '{TAB}' 200; Keys '{TAB}' 200; Keys '{TAB}' 200; Keys '{TAB}' 200; Keys ' ' 800   # search → sort → Clear all shown → row → its checkbox
Shot "$P-19-site-selected"
Keys '+{TAB}' 200; Keys '+{TAB}' 200; Keys '+{TAB}' 200; Keys '{ENTER}' 1200   # back over the row and Clear all shown: Clear selected (1)
Shot "$P-20-clear-confirm"
Keys '{ENTER}' 2500
Shot "$P-21-site-cleared"
Log "[$P] permissions log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'asked by|allowed for|blocked for|dismissed|camera on|camera off|stored choice|denied|forgot') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
Log "[$P] done"
