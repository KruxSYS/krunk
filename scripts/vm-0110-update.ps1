param([string]$Setup = '', [string]$Want = '0.11.0', [string]$Tag = 'up0110')
# Krunk 0.11.0 update path (from scripts/vm-0100-update.ps1): the installed copy (real data folder) finds $Want on its update
# server: the dot on ⋮, "Update available - Krunk $Want" first in the menu, which opens the update
# prompt; Update now downloads, verifies and installs it (0.10.0: a new libcef.dll) and Krunk comes
# back; then the version, hashes, the engine-file check and the log. With -Setup, that installer is
# installed first (/S): before publishing, a 0.9.0 --dev build that updates from staging through
# the ssh tunnel; after publishing, the release 0.9.0 against the live server.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = $Tag
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$data = Join-Path $env:LOCALAPPDATA 'Krunk'
function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { WindowRect $pw } else { $null }
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
if ($Setup) {
  $proc = Start-Process -FilePath $Setup -ArgumentList '/S' -PassThru
  $proc.WaitForExit(180000) | Out-Null
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Log "[$P] installed $Setup (exit $($proc.ExitCode))"
}
Log "[$P] installed $(Version), libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$t0 = Get-Date
Start-Sleep 4
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
for ($i = 0; $i -lt 100 -and -not (Select-String -Path "$data\krunk.log" -Pattern "$([regex]::Escape($Want)) is available" -Quiet); $i++) { Start-Sleep 1 }
Start-Sleep 1
Log "[$P] notice after $([int]((Get-Date) - $t0).TotalSeconds) s: $((Get-Content "$data\krunk.log" | Select-String 'update check' | Select-Object -Last 1).Line)"
Shot "$P-1-dot"
$c = ClientRect $A
Click ($c.Right - 20) 56 1200
Shot "$P-2-menu"
$m = PopupRectNow
if ($m) { Click ($m.Left + 80) ($m.Top + 22) 2000 }
$r = PopupRectNow
Shot "$P-3-prompt"
if ($r) { Click ($r.Right - 66) ($r.Top + 103) 3000 } else { Log "[$P] no prompt" }
$t0 = Get-Date
for ($i = 0; $i -lt 240 -and (Version) -ne $Want; $i++) { Start-Sleep 1 }
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
Start-Sleep 25
Log "[$P] version $(Version) after $([int]((Get-Date) - $t0).TotalSeconds) s; Krunk.dll $((Get-FileHash "$inst\Krunk.dll").Hash.Substring(0,16)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
Shot "$P-4-after"
Log "[$P] log: $((Get-Content "$data\krunk.log" | Select-String 'starting|release files|engine files|update') -join ' ## ')"
Log "[$P] WARN/ERROR: $(@(Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ').Count) $((Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ' | Select-Object -Last 3) -join ' ## ')"
# 0.11.0: Krunk's app identity (item 5): the registry key, the Start menu shortcut's ID, and the
# Windows media overlay with sound playing (the volume key shows it). Then, if this profile is
# signed in, the account card (item 9).
$reg = Get-ItemProperty 'HKCU:\Software\Classes\AppUserModelId\Krunk.Browser' -ErrorAction SilentlyContinue
Log "[$P] AppUserModelId key: DisplayName '$($reg.DisplayName)', IconUri '$($reg.IconUri)'"
$shell = New-Object -ComObject Shell.Application
$progs = $shell.Namespace([Environment]::GetFolderPath('Programs'))
$lnk = $progs.ParseName('Krunk.lnk')
Log "[$P] Start menu shortcut ID: '$(if ($lnk) { $lnk.ExtendedProperty('System.AppUserModel.ID') } else { 'no shortcut' })'"
Log "[$P] identity log: $((Get-Content "$data\krunk.log" | Select-String 'app identity') -join ' ## ')"
$A = MainWindow
if ($A -ne [IntPtr]::Zero) {
  Focus $A | Out-Null
  if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
    Start-Sleep 2
  }
  SafeGo 'http://localhost:8094/pipvol.html' 4000
  Click 700 300 300; Keys 'm' 600; Keys ' ' 800   # sound on, play
  [U]::keybd_event(0xAF, 0, 0, [UIntPtr]::Zero); [U]::keybd_event(0xAF, 0, 2, [UIntPtr]::Zero)   # volume up: the overlay
  Start-Sleep -Milliseconds 1500
  Shot "$P-5-media-overlay"
  Focus $A | Out-Null
  SafeGo 'krunk://settings#account' 3000
  Shot "$P-6-account-card"
}
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
