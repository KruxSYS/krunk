# Krunk 0.25.0 regression: automatic Picture-in-Picture on tab switches (0.24.2 behaviour) with
# the warm tabs and the new reveal. Setting autoPip on in a fresh profile.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'autopip'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p0250ap'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Copy-Item -Recurse C:\pipeline\krunk\sp-tpl\* $profile -ErrorAction SilentlyContinue
Log "[$P] settings: $(python C:\pipeline\krunk\setkv.py $profile '{\"autoPip\": true, \"checkUpdatesAutomatically\": false}')"
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1000, 700, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PipWindow {
  $pids = KrunkPids
  [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -notlike 'Krunk*' -and [TW]::Txt($_) -ne '' } | Select-Object -First 1
}
function Bar {
  $pids = KrunkPids
  $h = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($h) { WindowRect $h } else { $null }
}

function Floating { if (PipWindow) { 'floating' } else { 'none' } }
function LogLines([string]$pat) { @(Select-String -Path "$profile\Krunk\krunk.log" -Pattern $pat -ErrorAction SilentlyContinue).Count }
# Tab 1 plays (muted autoplay); leaving it sends its video to the floating window, coming back
# takes it out (0.24.0). A second playing tab never takes the window from the first (0.24.2).
SafeGo 'http://localhost:8094/pip.html#one' 4000
SafeKeys '^t' 1500
Log "[$P] tab 1 left (Ctrl+T): $(Floating), auto lines $(LogLines 'automatic picture-in-picture')"
Shot "$P-1-left"
SafeGo 'http://localhost:8094/pip.html#one' 4000
SafeKeys '^t' 1500
Log "[$P] tab 2 (also playing) left: $(Floating), holder lines $(LogLines 'has the floating window')"
Shot "$P-2-second-left"
SafeKeys '^1' 2000
Log "[$P] back to tab 1: $(Floating)"
Shot "$P-3-back"
# Rapid switches over the three tabs must not leave a stray floating window or a wrong page.
for ($i = 0; $i -lt 6; $i++) { SafeKeys '^{TAB}' 250 }
SafeKeys '^1' 2000
Log "[$P] after rapid switching, on tab 1: $(Floating), page '$([TW]::Txt($A))'"
Shot "$P-4-after-rapid"
Log "[$P] WARN/ERROR lines: $(LogLines ' WARN | ERROR ')"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
