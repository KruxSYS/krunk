# Krunk 0.11.1 VM flow: the smaller fixes, on one fresh profile with the 0.11.0 bookmarks seed
# (vm-0110-seed.py) and an Accept-Language override (it starts tabs at about:blank first).
#   overflow  » opens tall; Later (a short folder) and Back: the list has its first height again
#   blank     a new tab and two pages: the Back history menu has no about:blank
#   ext       krunk://extensions (spacing)
# Needs serve.ps1 on 8094 (started here). Screenshots "small-*"; the popup heights in run.log.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = 'small'
$profile = "$K\p0111s"
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
}
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
}
function StopKrunk {
  foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1200 } }
  Start-Sleep 2
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
}
function PopupRect {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $h = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($h) { $r = WindowRect $h; "$($r.Left),$($r.Top) $($r.Right - $r.Left)x$($r.Bottom - $r.Top)" } else { 'none' }
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
StartKrunk; StopKrunk
Log "[$P] seed: $(python "$K\vm-0110-seed.py" $profile)"
Log "[$P] setkv: $(python "$K\setkv.py" $profile '{\"headers\": {\"acceptLanguage\": \"de-DE,de;q=0.9,en;q=0.5\"}}')"
StartKrunk
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$wDip = ($c.Right - $c.Left) / $c.Scale
$barY = Y 90

# ---- overflow: », then Later, then Back.
Click (X ($wDip - 22)) $barY 900
Log "[$P] » opened: $(PopupRect)"
Shot "$P-1-overflow"
Keys '{END}' 150; Keys '{UP}' 150; Keys '{ENTER}' 900
Log "[$P] Later: $(PopupRect)"
Shot "$P-1-later"
Keys '{BACKSPACE}' 900
Log "[$P] Back to »: $(PopupRect)"
Shot "$P-1-back-to-overflow"
Keys '{ESC}' 400

# ---- about:blank: a new tab, then a page; Back's state and its history menu.
SafeKeys '^t' 1500
Shot "$P-2-new-tab"
SafeGo 'http://localhost:8094/marker.html' 1500
SafeGo 'http://localhost:8094/light.html' 1500
RightClick (X 20) (Y 56) 900
Log "[$P] Back menu: $(PopupRect)"
Shot "$P-2-back-menu"
Keys '{ESC}' 400
SafeKeys '^t' 1500
SafeGo 'http://localhost:8094/marker.html' 1500
Shot "$P-2-new-tab-one-page"

# ---- the extensions page.
SafeGo 'krunk://extensions' 2000
Shot "$P-3-extensions"
StopKrunk
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR|popup: shown without|history' | Select-Object -Last 15) -join ' ## ')"
Log "[$P] done"
