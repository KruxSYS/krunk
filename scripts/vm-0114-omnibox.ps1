param([string]$Label = 'now')
# Krunk 0.11.4 probe: clicking an address-bar suggestion must open it (operator: it only
# closed the list). History has example.com; a new tab types "exam" and clicks the first row.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "omni-$Label"
$K = 'C:\pipeline\krunk'
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRect {
  $pids = KrunkPids
  $h = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($h) { WindowRect $h } else { $null }
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$profile = "$K\p0114o"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'https://example.com/' 3000
SafeKeys '^t' 1500
SafeKeys 'exam' 1500
$r = PopupRect
Log "[$P] suggestions: $(if ($r) { "$($r.Left),$($r.Top) $($r.Right - $r.Left)x$($r.Bottom - $r.Top)" } else { 'none' })"
Shot "$P-1-suggestions"
if ($r) { Click ($r.Left + 120) ($r.Top + 22) 3000 }
Log "[$P] after the click: title '$([TW]::Txt($A))'; popup $(if (PopupRect) { 'still open' } else { 'closed' })"
Shot "$P-2-after-click"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'address bar|suggest|popup|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
Log "[$P] done"
