# Krunk 0.3.0 acceptance 4(f): every window comes back after a restart, with its tabs, place
# and maximized state; an off-screen window is moved back onto the monitor. Fresh profile.
# Phase "save": two windows, Exit. Phase "restore": start, check, close, edit the session to
# put a window off screen (done from Linux between the phases), start again.
param([string]$Phase = 'save')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "sess-$Phase"
$profile = 'C:\pipeline\krunk\p030s'
$env:LOCALAPPDATA = $profile
function Start-Krunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}
if ($Phase -eq 'save') {
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
  Start-Krunk
  $A = MainWindow
  [U]::MoveWindow($A, 20, 30, 700, 600, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log "[$P] no focus"; throw 'no focus' }
  SafeGo 'https://example.com/' 3000
  SafeKeys '^t' 1500; SafeGo 'krunk://about' 2500
  SafeKeys '^n' 3000
  $B = [U]::GetForegroundWindow()
  [U]::MoveWindow($B, 560, 120, 700, 560, $true) | Out-Null; Start-Sleep 1
  SafeGo 'http://localhost:8092/move.html?w=b' 3000
  SafeKeys '^t' 1500; SafeGo 'https://example.com/?w=b2' 3000
  # Window C, maximized.
  SafeKeys '^n' 3000
  $C = [U]::GetForegroundWindow()
  [U]::ShowWindow($C, 3) | Out-Null; Start-Sleep 1
  SafeGo 'krunk://settings' 2500
  # Back to A: the window used last.
  if (-not (Focus $A)) { throw 'no focus' }
  Start-Sleep 1
  Shot "$P-1-three-windows"
  KrunkWindows $P
  # Exit from the menu (all windows close at once).
  $ca = ClientRect $A
  Click ([int]($ca.Right - 22)) ([int]($ca.Top + 56)) 1200
  Shot "$P-2-menu"
  # Exit is the last item: popup top = toolbar bottom (76 DIP) + 6 + 4 groups of items and separators.
  Click ([int]($ca.Right - 160)) ([int]($ca.Top + (76 + 504) * $ca.Scale)) 3000
  Start-Sleep 4
  Log "[$P] Krunk processes after Exit: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
} else {
  Start-Krunk
  Start-Sleep 3
  Shot "$P-1-restored"
  KrunkWindows $P
  Get-Process Krunk -ErrorAction SilentlyContinue | Out-Null
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
  Start-Sleep 4
  Log "[$P] Krunk processes after closing: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
}
Copy-Item "$profile\Krunk\browser.db" "C:\pipeline\krunk\p030s-$Phase.db" -Force
Get-Content "$profile\Krunk\krunk.log" | Select-String 'session|window|WARN|ERROR' | ForEach-Object { Log "[$P] log: $_" }
