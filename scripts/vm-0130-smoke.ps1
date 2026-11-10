# Krunk 0.13.0 smoke check on the VM (the --dev build in C:\pipeline\krunk\appdev, site
# http://127.0.0.1:18080 = the staging stack through `ssh -R 18080:dind:<stage port>`):
#   1 Settings → Account → Sign in: the code with the server's QR code next to it (screenshot)
#   2 the code is approved from Linux (scripts/vm-approve-link.sh, waits for code.txt) → signed in
#   3 Add a device: a new tab with <site>/account/#add-device (screenshot, its address logged)
# -Phase signin runs 1-2 and leaves Krunk open; -Phase add runs 3 and closes it. Buttons are
# clicked at screen coordinates read from the screenshots (the window at 0,0, 1280×760, 96 DPI):
# Sign in (412,346); Add a device is passed in after the signed-in screenshot.
param([string]$Phase = 'signin', [int]$AddX = 0, [int]$AddY = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'smoke0130'
$K = 'C:\pipeline\krunk'
$App = "$K\appdev"
$profile = "$K\p0130"
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }

if ($Phase -eq 'signin') {
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item "$K\approved", "$K\code.txt" -Force -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
Set-Content -Encoding ascii "$K\patch.json" '{"checkUpdatesAutomatically":false}'
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'krunk://settings#account' 3000
Shot "$P-0-signed-out"
# 1: Sign in. Krunk opens the sign-in page in a new tab; its address holds the code.
Click 412 346 5000
SafeKeys '^l' 300; SafeKeys '^c' 300
$addr = Get-Clipboard
$code = ([regex]::Match($addr, 'code=([0-9A-Z-]{9})')).Groups[1].Value
Log "[$P] opened tab: $($addr -replace 'code=.*', 'code=<code>') (code read: $([bool]$code))"
Set-Content -Encoding ascii "$K\code.txt" $code
SafeKeys '{ESC}' 200; SafeKeys '^1' 1500
Shot "$P-1-code-and-qr"
Log "[$P] account lines: $(KLog 'account:')"
# 2: approved from Linux.
for ($i = 0; $i -lt 150 -and -not (Test-Path "$K\approved"); $i++) { Start-Sleep 1 }
Log "[$P] approved flag after $i s"
Start-Sleep 12
Shot "$P-2-signed-in"
Log "[$P] account lines: $(KLog 'account:')"
} else {
$A = MainWindow
Focus $A | Out-Null
# 3: Add a device.
Click $AddX $AddY 4000
SafeKeys '^l' 300; SafeKeys '^c' 300
Log "[$P] Add a device opened: $(Get-Clipboard)"
SafeKeys '{ESC}' 600
Shot "$P-3-add-device-tab"
if (Focus $A) { SafeKeys '%{F4}' 2500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Log "[$P] WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
}
Log "[$P] $Phase done"
