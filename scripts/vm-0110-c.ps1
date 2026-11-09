# Krunk 0.11.0 VM flow C: "Remain unlocked this session" (item 7) on two Krunk instances side
# by side (two copies of one profile with a vault, auto-lock 1 minute): instance ON unlocks
# with the box checked (the default), instance OFF unchecks it. While both wait ~95 s, the
# hung-page prompt (item 14) runs in ON. Then: ON still unlocked, OFF locked by inactivity, and
# OFF's form remembers "unchecked". Screenshots "c-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'c'
$on = "$K\p0110c-on"; $off = "$K\p0110c-off"
$master = 'Krunk-test-master-1'
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
}
function Launch([string]$profile, [int]$x) {
  $before = @(Frames)
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  $h = [IntPtr]::Zero
  for ($i = 0; $i -lt 60 -and $h -eq [IntPtr]::Zero; $i++) {
    Start-Sleep -Milliseconds 500
    $h = @(Frames | Where-Object { $before -notcontains $_ }) | Select-Object -First 1
    if (-not $h) { $h = [IntPtr]::Zero }
  }
  Start-Sleep 5
  [U]::MoveWindow($h, $x, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  $h
}
function LogOf([string]$profile, [string]$pattern) { @(Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function Popups {
  $pids = KrunkPids
  @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' })
}
# krunk://passwords' unlock form: the field (625,302 at a window at x=0), then Tab to the box.
function Unlock([IntPtr]$h, [bool]$remain, [string]$tag) {
  Focus $h | Out-Null
  SafeGo 'krunk://passwords' 3000
  $c = ClientRect $h
  Click ($c.Left - 8 + 625) 302 400
  if (-not $remain) { Keys '{TAB}' 200; Keys ' ' 300; Keys '+{TAB}' 200 }
  Shot "$P-1-$tag-form"
  Keys $master 200; Keys '{ENTER}' 2500
  Shot "$P-2-$tag-unlocked"
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Remove-Item -Recurse -Force $on, $off -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $on | Out-Null
$A = Launch $on 0
Focus $A | Out-Null
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-0-vault-created"
foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1500 } }
Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] setkv: $(python "$K\setkv.py" $on '{\"vaultAutolockMinutes\": 1}')"
Copy-Item -Recurse $on $off

$B = Launch $off 0
Unlock $B $false 'off'
$tOff = Get-Date
$A = Launch $on 0
Unlock $A $true 'on'
$tOn = Get-Date

# ---- 14 in ON: a page that hangs; the prompt, then Close page and the crash page.
Focus $A | Out-Null
SafeKeys '^t' 600; SafeGo 'http://localhost:8094/hang.html' 3000
Click 640 400 300; Keys 'x' 200
$s = Get-Date
for ($i = 0; $i -lt 30 -and @(Popups).Count -eq 0; $i++) { Start-Sleep 1 }
Log "[$P] hang prompt after $([int]((Get-Date) - $s).TotalSeconds) s: popups $(@(Popups).Count); $((LogOf $on 'not responding') -join ' ## ')"
Shot "$P-3-not-responding-prompt"
Keys '+{TAB}' 200; Keys '{ENTER}' 3500                  # Close page (Wait is the default)
Log "[$P] after Close page: title '$([TW]::Txt($A))'; $((LogOf $on 'page is closed|renderer ended') -join ' ## ')"
Shot "$P-4-crash-page"
Keys '{ENTER}' 3000                                      # Reload (the page focuses it)
Shot "$P-5-after-reload-click"

# ---- 7: both have waited; OFF must have locked, ON not.
for ($i = 0; $i -lt 100 -and (LogOf $off 'vault locked \(inactivity\)').Count -eq 0; $i++) { Start-Sleep 1 }
Log "[$P] OFF: locked by inactivity $([int]((Get-Date) - $tOff).TotalSeconds) s after unlocking: $((LogOf $off 'vault locked').Count)"
$wait = 75 - ((Get-Date) - $tOn).TotalSeconds; if ($wait -gt 0) { Start-Sleep ([int]$wait) }
Log "[$P] ON: $([int]((Get-Date) - $tOn).TotalSeconds) s after unlocking, inactivity locks $((LogOf $on 'vault locked \(inactivity\)').Count); $((LogOf $on 'vault unlocked') -join ' ## ')"
Focus $A | Out-Null; SafeKeys '^1' 500; SafeKeys '{F5}' 2000
Shot "$P-6-on-still-unlocked"
Focus $B | Out-Null; SafeKeys '{F5}' 2500
Shot "$P-7-off-locked-remembers-unchecked"
foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1500 } }
Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
