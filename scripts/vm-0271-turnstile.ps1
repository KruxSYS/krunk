# Krunk 0.27.1 Turnstile check: a fresh copy of lp-base on build -App, then Cloudflare challenge
# pages, each given 25 s; shots ts-<Tag>-<n>. Extra engine/profile tweaks via -Ext (an unpacked
# extension folder loaded with --load-extension is not used: Krunk installs from file only).
param([string]$App = 'app270r', [string]$Tag = 'a', [string]$Ext = '', [switch]$Private)
. C:\pipeline\krunk\lib.ps1
# A leftover Windows Firewall prompt ("Windows Security") takes the keystrokes: Esc it.
for ($i = 0; $i -lt 3; $i++) {
  $fw = [U]::FindWindow([NullString]::Value, 'Windows Security')
  if ($fw -eq [IntPtr]::Zero) { break }
  [U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($fw)
  [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 500; Keys '{ESC}' 1000
}
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$P = "lp-ts-$Tag"
Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$P"
$env:LOCALAPPDATA = "$K\$P"
Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 6
[U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
[U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 800
Click 640 30 300
# A fast build of a published version gets the Repair popup: Later (as vm-0253-start.ps1).
if ($App -notlike '*r' -and $App -ne 'app260rel') { Start-Sleep 16; Click 1127 175 800; Click 640 30 300 }
if ($Ext) {
  # Install from file (krunk://extensions, 1280 wide), accept the prompt with Enter.
  Go 'krunk://extensions' 3000
  Click 904 144 2500
  Keys $Ext 300; Keys '{ENTER}' 6000
  Shot "ts-$Tag-prompt"
  Keys '{ENTER}' 4000
  Click 640 30 300
  Shot "ts-$Tag-installed"
}
if ($Private) { Keys '^+n' 2500; $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Click 640 30 300 }
# The interactive checkbox (nopecha's managed challenge): click it, then look twice.
Go 'https://nopecha.com/demo/cloudflare' 20000; Shot "ts-$Tag-1"
Click 213 441 15000; Shot "ts-$Tag-2"
Start-Sleep 15; Shot "ts-$Tag-3"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 4; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
