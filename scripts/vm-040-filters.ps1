# Krunk 0.4.0 milestone 8: filter lists and My filters on krunk://adblock, without restarts.
# vm-www/8092/own.html probes three requests: probe.png (blocked by a filter of one's own),
# buttons.github.io/buttons.js (only in Fanboy's Social list, added by its address) and an
# EasyList-only ad host. Started with --log-blocked; the window title carries the page's
# result, krunk.log which requests were blocked. Fresh profile. Screenshots "flt-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'flt'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p040f'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
function StartKrunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -ArgumentList '--log-blocked' -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
  $script:c = ClientRect $A
  $script:page = [int]($c.Top + 104 * $c.Scale)
  # krunk://adblock's content column (960 CSS px wide, centred, 24 px padding).
  $script:col = [int]($c.Left + ((($c.Right - $c.Left) / $c.Scale - 960) / 2 + 24) * $c.Scale)
}
function At([int]$x, [int]$y) { @([int]($col + $x * $c.Scale), [int]($page + $y * $c.Scale)) }
function Probe([string]$label) {
  Log "[$label] ---- own.html"
  SafeGo 'http://localhost:8092/own.html' 5000
  Log "[$label] title: $([TW]::Txt($A))"
  Shot "$P-$label"
}
function Adblock([string]$tab) {
  SafeGo 'krunk://adblock' 2500
  $x = if ($tab -eq 'lists') { 224 } else { 328 }
  $pt = At $x 89; Click $pt[0] $pt[1] 2000
}
StartKrunk
Probe '1-before'

# My filters: one rule, Ctrl+S.
Adblock 'filters'
$pt = At 400 300; Click $pt[0] $pt[1] 300
Keys '! test{ENTER}||localhost{^}*/probe.png{ENTER}' 300
Keys '^s' 6000
Shot "$P-2-my-filters"
Probe '3-own-filter'

# Filter lists: an address that does not exist, then Fanboy's Social list.
Adblock 'lists'
Shot "$P-4-lists"
$pt = At 400 186; Click $pt[0] $pt[1] 300
Keys 'https://ublockorigin.github.io/uAssets/filters/nope.txt{ENTER}' 10000
Shot "$P-5-add-missing"
$pt = At 400 186; Click $pt[0] $pt[1] 300
Keys '^a' 100
Keys 'https://easylist.to/easylist/fanboy-social.txt{ENTER}' 15000
Shot "$P-6-added"
Probe '7-added-list'
# The blocked list of this page (shield, "Show all"): which filter and list blocked each.
Click ([int]($c.Right - 56 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 1500
Keys '{TAB}{TAB}' 300; Keys '{ENTER}' 4000
Shot "$P-8-blocked-page"

# EasyList off (the first list's checkbox: two tabs after the address field).
Adblock 'lists'
$pt = At 400 186; Click $pt[0] $pt[1] 300
Keys '{TAB}{TAB}' 300; Keys ' ' 8000
Shot "$P-9-easylist-off"
Probe '10-easylist-off'

# Everything is still so after a restart.
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
for ($i = 0; $i -lt 40 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
Get-Content "$profile\Krunk\krunk.log" | Select-String 'blocked|redirected|filter|WARN|ERROR' | ForEach-Object { Log "[log first run] $_" }
StartKrunk
Probe '11-after-restart'
Adblock 'lists'
Shot "$P-12-lists-after-restart"
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-ChildItem "$profile\Krunk\filters" | ForEach-Object { Log "[files] $($_.Name) $($_.Length)" }
Get-Content "$profile\Krunk\filters\state.json" | ForEach-Object { Log "[state] $_" }
Get-Content "$profile\Krunk\krunk.log" | Select-String 'blocked|redirected|filter|WARN|ERROR' | ForEach-Object { Log "[log second run] $_" }
