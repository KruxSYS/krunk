param([string]$Setup = '', [string]$Want = '0.11.0', [string]$Tag = 'up0110b')
# Krunk 0.11.0 update path, short (after the shortcut-stamp crash fix): -Setup installs that
# installer first (its shortcuts have no app ID, so 0.11.0's first start stamps them). Then the
# in-app update from ⋮; the first start of 0.11.0 must survive 35 s with no crash event, a second
# start (stamped shortcuts) 15 s; the registry key, the shortcut's ID and one media-overlay shot.
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
function Crashes([datetime]$since) { @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000; StartTime = $since } -ErrorAction SilentlyContinue | Where-Object { $_.Message -like '*Krunk.exe*' }).Count }
function Alive { @(Get-Process Krunk -ErrorAction SilentlyContinue).Count -gt 0 -and (MainWindow) -ne [IntPtr]::Zero }
$t00 = Get-Date
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
if ($Setup) {
  $proc = Start-Process -FilePath $Setup -ArgumentList '/S' -PassThru
  $proc.WaitForExit(180000) | Out-Null
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Log "[$P] installed $Setup (exit $($proc.ExitCode))"
}
$shell = New-Object -ComObject Shell.Application
function LnkId { $l = $shell.Namespace([Environment]::GetFolderPath('Programs')).ParseName('Krunk.lnk'); if ($l) { "'$($l.ExtendedProperty('System.AppUserModel.ID'))'" } else { 'no shortcut' } }
Log "[$P] installed $(Version); Start menu shortcut ID before: $(LnkId)"
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
for ($i = 0; $i -lt 100 -and -not (Select-String -Path "$data\krunk.log" -Pattern "$([regex]::Escape($Want)) is available" -Quiet); $i++) { Start-Sleep 1 }
$c = ClientRect $A
Click ($c.Right - 20) 56 1200
$m = PopupRectNow
if ($m) { Click ($m.Left + 80) ($m.Top + 22) 2000 }
$r = PopupRectNow
if ($r) { Click ($r.Right - 66) ($r.Top + 103) 3000 } else { Log "[$P] no prompt" }
$t0 = Get-Date
for ($i = 0; $i -lt 240 -and (Version) -ne $Want; $i++) { Start-Sleep 1 }
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
$start1 = Get-Date
Start-Sleep 5
Log "[$P] version $(Version) after $([int]($start1 - $t0).TotalSeconds) s; first start alive after 5 s: $(Alive); crash events since the run began: $(Crashes $t00); Krunk.dll $((Get-FileHash "$inst\Krunk.dll").Hash.Substring(0,16)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
Shot "$P-1-first-start-35s"
Log "[$P] first start log: $((Get-Content "$data\krunk.log" | Select-String 'starting|app identity|engine files|release files|WARN|ERROR') -join ' ## ')"
$reg = Get-ItemProperty 'HKCU:\Software\Classes\AppUserModelId\Krunk.Browser' -ErrorAction SilentlyContinue
Log "[$P] AppUserModelId key: '$($reg.DisplayName)' '$($reg.IconUri)'; Start menu shortcut ID: $(LnkId)"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Log "[$P] running app: $((Get-Content "$data\krunk.log" | Select-String 'starting' | Select-Object -Last 1).Line); exe $((Get-Item "$inst\Krunk.exe").VersionInfo.ProductVersion)"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Log "[$P] done (live, short)"; return
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 4
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
SafeGo 'http://localhost:8094/pipvol.html' 3500
Click 700 300 300; Keys 'm' 500; Keys ' ' 800
[U]::keybd_event(0xAF, 0, 0, [UIntPtr]::Zero); [U]::keybd_event(0xAF, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 1500
Shot "$P-2-media-overlay"
Start-Sleep 6
Log "[$P] second start alive after 15 s: $(Alive); crash events since the run began: $(Crashes $t00)"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
