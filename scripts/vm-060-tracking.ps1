# Krunk 0.6.0 milestone 8 (R17): tracking protection. Standard: the cryptominer and the
# tracker pixel are blocked, the fingerprinting CDN is not, a cross-site request carries only
# the origin as Referer, utm_*/fbclid are stripped, a redirect hop is noted. Strict: the
# fingerprinting script is blocked too and no Referer leaves the site. The shield exception
# turns it all off for the site. Pages: www/8094/tracking.html and the /headers echo on 8443.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "trk-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
# serve.ps1 gained /bounce: restart the servers.
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep 1
foreach ($prefix in 'http://*:8094/', 'https://*:8443/') {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',$prefix
}
Start-Sleep 2
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
function PopupRectNow {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1; Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue }
# Sets the level through the settings search (no coordinates): the first radio gets the
# focus with Tab, the arrow keys move the selection (and save it).
function SetLevel([string]$arrows) {
  SafeGo 'krunk://settings#privacy' 3000
  Click (X 968) (Y 146) 300; Keys 'Tracking protection level' 600
  Keys '{TAB}' 300; Keys $arrows 1200
}
# A fresh profile downloads its filter lists a minute after start; the level change below
# would queue its rebuild behind that, so let the first update finish first.
for ($i = 0; $i -lt 300 -and -not (Get-Content "$profile\Krunk\krunk.log" | Select-String 'filter lists checked'); $i++) { Start-Sleep -Milliseconds 500 }
Log "[$P] first list update done after $i ticks"
# ---- Standard
SafeGo 'http://localhost:8094/tracking.html' 4500
Shot "$P-1-standard-probe"
Click (X 200) (Y 160) 4000       # the cross-site echo link (utm_*, fbclid)
Shot "$P-2-standard-echo"
Keys '%{LEFT}' 2500
Click (X 200) (Y 198) 4000       # the redirect hop
Shot "$P-3-after-bounce"
SafeGo 'krunk://settings#privacy' 3500
Keys '{PGDN}' 1000
Shot "$P-4-settings-standard"
# ---- Strict
SetLevel '{DOWN}'
Shot "$P-5-settings-strict"
# The engine is rebuilt with the fingerprinting list; on a fresh profile the first list
# update (60 s after start) may be running, and the rebuild queues behind it.
$before = @(Get-Content "$profile\Krunk\krunk.log" | Select-String 'filter engine rebuilt').Count
for ($i = 0; $i -lt 120 -and @(Get-Content "$profile\Krunk\krunk.log" | Select-String 'filter engine rebuilt').Count -le $before; $i++) { Start-Sleep -Milliseconds 500 }
Log "[$P] engine rebuilt after Strict: $(@(Get-Content "$profile\Krunk\krunk.log" | Select-String 'filter engine rebuilt').Count -gt $before) ($i ticks)"
Start-Sleep 1
SafeGo 'http://localhost:8094/tracking.html' 4500
Shot "$P-6-strict-probe"
Click (X 200) (Y 160) 4000
Shot "$P-7-strict-echo"
# ---- The shield exception: everything off for localhost.
SafeGo 'http://localhost:8094/tracking.html' 4000
Click (X 1215) (Y 56) 1500       # the shield
$pr = PopupRectNow; Log "[$P] shield popup: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"
Shot "$P-8-shield"
if ($pr) { Click ($pr.Left + 111) ($pr.Top + 270) 3500 }  # "Show all N blocked addresses" -> krunk://adblock
Shot "$P-8b-blocked-with-lists"
Keys '%{LEFT}' 2500
Click (X 1215) (Y 56) 1500
$pr = PopupRectNow
if ($pr) { Click ($pr.Right - 34) ($pr.Top + 34) 3500 }   # the site switch (reloads the page)
Shot "$P-9-exception-probe"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'tracking protection|bounce trackers|blocked (script|image)') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
Log "[$P] done"
