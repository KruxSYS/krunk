# Krunk 0.25.4 VM smoke (VPN kill switch): starts the fast build (C:\pipeline\krunk\app254) with
# the DevTools port 9222; scripts/perf/d0254.mjs drives it over CDP (ssh -L 19870:...:9222).
#   -Step start [-Fresh]   (re)start Krunk (-Fresh: a new profile)
#   -Step dialog           type the dead WireGuard profile's path into the open file dialog
#   -Step shot -Name N     screenshot
param([string]$Step = 'start', [switch]$Fresh, [string]$Name = 'shot')
. C:\pipeline\krunk\lib.ps1
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0254"
switch ($Step) {
  'start' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 2
    if ($Fresh) { Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $prof | Out-Null }
    $env:LOCALAPPDATA = $prof; $env:KRUNK_DEVTOOLS_PORT = '9222'
    Start-Process -FilePath "$Root\app254\Krunk.exe" -WorkingDirectory "$Root\app254"
    for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null
    Start-Sleep 22
    Click 1127 175 500   # a fast build's "repair" prompt: Later
    Log "started (fresh=$Fresh)"
  }
  'dialog' { Keys "$Root\dead-wg.conf{ENTER}" 2500; Log 'typed the profile path' }
  'shot' { Shot "v0254-$Name" }
}
