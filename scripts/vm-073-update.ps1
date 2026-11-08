# Krunk 0.7.3 release check: the installed 0.7.2 (real data folder) finds 0.7.3 on the live server:
# the dot on ⋮, "Update available – Krunk 0.7.3" first in the menu, which opens the update prompt;
# Update now installs 0.7.3 and Krunk comes back. Then the version, hashes and the log.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'up073'
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
Log "[$P] installed $(Version)"
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$t0 = Get-Date
Start-Sleep 4
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
for ($i = 0; $i -lt 40 -and -not (Select-String -Path "$data\krunk.log" -Pattern 'is available' -Quiet); $i++) { Start-Sleep 1 }
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
for ($i = 0; $i -lt 240 -and (Version) -ne '0.7.3'; $i++) { Start-Sleep 1 }
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
Start-Sleep 25
Log "[$P] version $(Version) after $([int]((Get-Date) - $t0).TotalSeconds) s; Krunk.dll $((Get-FileHash "$inst\Krunk.dll").Hash.Substring(0,16)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
Shot "$P-4-after"
Log "[$P] log: $((Get-Content "$data\krunk.log" | Select-String 'starting|release files|engine files') -join ' ## ')"
Log "[$P] WARN/ERROR: $(@(Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ').Count) $((Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ' | Select-Object -Last 3) -join ' ## ')"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
