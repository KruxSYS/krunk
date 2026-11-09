# Krunk 0.11.0 VM flow B: "Remain unlocked this session" (item 7), a hung and a slow page
# (14), the cookie list (10) and the PiP volume slider (6), on one fresh profile with the
# auto-lock set to 1 minute. Needs serve.ps1 on 8094 and slowserve.py on 8104 (started here).
# Screenshots "b-*"; vault, hang and source lines of krunk.log in run.log.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'b'
$profile = "$K\p0110b"
$master = 'Krunk-test-master-1'
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
}
if (-not (Get-NetTCPConnection -LocalPort 8104 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process python -WindowStyle Hidden -ArgumentList "$K\slowserve.py",'8104','30'
}
function StartKrunk([bool]$fresh) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  if ($fresh) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $h = MainWindow
  [U]::MoveWindow($h, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $h)) { Log '[setup] could not focus'; throw 'no focus' }
  $h
}
function Quit { foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1500 } }; Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force }
function KrunkLog([string]$pattern) { @(Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PipWindow {
  $pids = KrunkPids
  [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -notlike 'Krunk*' -and [TW]::Txt($_) -ne '' } | Select-Object -First 1
}
function Popups {
  $pids = KrunkPids
  @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' })
}
function RectText($r) { if ($r) { "$($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { 'none' } }
# The unlock form of krunk://passwords: the password field, then (Tab) the checkbox.
function UnlockOnPage([bool]$remain) {
  SafeGo 'krunk://passwords' 3000
  Click 640 371 400
  if (-not $remain) { Keys '{TAB}' 200; Keys ' ' 300; Keys '+{TAB}' 200 }
  Shot "$P-7-unlock-form-remain-$remain"
  Keys $master 200; Keys '{ENTER}' 2500
}

# ---- setup: a vault, then the auto-lock at 1 minute.
$A = StartKrunk $true
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-0-vault-created"
Quit
Log "[$P] setkv: $(python "$K\setkv.py" $profile '{\"vaultAutolockMinutes\": 1}')"

# ---- 7a: unlock with "Remain unlocked this session" (default on), then 80 s of other tests.
$A = StartKrunk $false
UnlockOnPage $true
$t1 = Get-Date
Shot "$P-7-unlocked-remain"

# ---- 6: the PiP volume slider inside the floating window.
SafeGo 'http://localhost:8094/pipvol.html' 4000
Click 700 300 300; Keys 'm' 800              # sound on (the page's own key)
SafeKeys '%p' 1500
$pw = PipWindow
if ($pw) {
  $pr = WindowRect $pw
  Log "[$P] floating window $(RectText $pr); popups before hover: $(@(Popups).Count)"
  MoveTo ([int](($pr.Left + $pr.Right) / 2)) ([int](($pr.Top + $pr.Bottom) / 2)); Start-Sleep -Milliseconds 800
  Log "[$P] pointer over the video: popups $(@(Popups).Count) (no bar)"
  Shot "$P-6-video-hover-no-bar"
  MoveTo ($pr.Right - 58) ($pr.Bottom - 18); Start-Sleep -Milliseconds 900
  $sl = Popups | Select-Object -First 1
  Log "[$P] pointer on the mute button: slider $(if ($sl) { RectText (WindowRect $sl) } else { 'none' }), page '$([TW]::Txt($A))'"
  Shot "$P-6-slider-on-mute-hover"
  Wheel ($pr.Right - 58) ($pr.Bottom - 18) -3
  Start-Sleep -Milliseconds 700
  Log "[$P] wheel up 3: page '$([TW]::Txt($A))'"
  Shot "$P-6-wheel"
  MoveTo ([int](($pr.Left + $pr.Right) / 2)) ([int]($pr.Top + 30)); Start-Sleep -Milliseconds 1500
  Log "[$P] pointer away from the button: popups $(@(Popups).Count)"
  Focus $A | Out-Null
  SafeKeys '%p' 1500
} else { Log "[$P] no floating window" }

# ---- 10: the cookie list with real (local) sites.
SafeGo 'krunk://storage' 3000
Shot "$P-10-storage"
Click 640 120 300; Keys '127' 800
Shot "$P-10-storage-search"

# ---- 14: a slow load and a hung page do not hold up the rest.
SafeKeys '^t' 600; SafeGo 'http://127.0.0.1:8104/slow' 600     # answers after 30 s
$s0 = Get-Date
SafeKeys '^1' 400; SafeKeys '^l' 100; SafeKeys 'typing while another tab loads' 100
Log "[$P] slow tab loading: switched back and typed in $([int]((Get-Date) - $s0).TotalMilliseconds) ms"
Shot "$P-14-typing-while-slow-load"
SafeKeys '{ESC}' 200; SafeKeys '{ESC}' 200
SafeKeys '^t' 600; SafeGo 'http://localhost:8094/hang.html' 2500    # hangs 1 s after loading
Click 640 400 300                                   # input it never handles
$s0 = Get-Date
SafeKeys '^1' 500
SafeKeys '^l' 100; SafeKeys 'still smooth' 100
Log "[$P] hung tab: switched away and typed in $([int]((Get-Date) - $s0).TotalMilliseconds) ms"
Shot "$P-14-other-tab-while-hung"
SafeKeys '{ESC}' 200; SafeKeys '{ESC}' 200
Wheel 640 400 -5
SafeKeys '^3' 600                                   # the hung tab
Click 640 400 300
Start-Sleep 18
Log "[$P] after 18 s on the hung tab: popups $(@(Popups).Count); log '$((KrunkLog 'not responding') -join ' ## ')'"
Shot "$P-14-not-responding-prompt"
Keys '+{TAB}' 200; Keys '{ENTER}' 3000                # Close page (Wait is the default)
Shot "$P-14-closed-crash-page"
Log "[$P] after Close page: title '$([TW]::Txt($A))'"

# ---- 7a check: still unlocked after more than a minute (the auto-lock is 1 minute).
$idle = [int]((Get-Date) - $t1).TotalSeconds
if ($idle -lt 75) { Start-Sleep (75 - $idle) }
Log "[$P] remain on, $([int]((Get-Date) - $t1).TotalSeconds) s after unlocking: inactivity locks $((KrunkLog 'vault locked \(inactivity\)').Count)"
SafeGo 'krunk://passwords' 2500
Shot "$P-7-still-unlocked-after-75s"
Quit

# ---- 7b: unchecked: the auto-lock applies; the choice is remembered.
$A = StartKrunk $false
UnlockOnPage $false
$t2 = Get-Date
Shot "$P-7-unlocked-not-remain"
for ($i = 0; $i -lt 100 -and (KrunkLog 'vault locked \(inactivity\)').Count -eq 0; $i++) { Start-Sleep 1 }
Log "[$P] remain off: locked by inactivity after $([int]((Get-Date) - $t2).TotalSeconds) s ($((KrunkLog 'vault locked').Count) lock lines)"
SafeGo 'krunk://passwords' 2500
Shot "$P-7-locked-again-remembered-off"
Log "[$P] vault log: $((KrunkLog 'vault') -join ' ## ')"
Quit
Log "[$P] hang/crash log: $((Get-Content "$profile\Krunk\krunk.old.log" -ErrorAction SilentlyContinue | Select-String 'respond|renderer ended|closed|WARN|ERROR' | Select-Object -Last 12) -join ' ## ')"
Log "[$P] done"
