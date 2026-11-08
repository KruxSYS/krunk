# Krunk 0.7.0 verification (R23): the engine's own command line with one --disable-features
# holding all 12 names. Krunk opens no chrome:// page but chrome://gpu (through krunk://gpu's
# button); its "Command Line" row is the browser process's command line, the same one
# chrome://version shows. Also the browser and GPU processes' command lines from Windows.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'cmdline'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = "$K\p070v"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$wanted = 'GlicActorUi', 'AutofillActorMode', 'LensOverlay', 'KillOnInvalidNavigationHeaders', 'AutofillServerCommunication', 'MediaRouter',
  'OptimizationHints', 'InterestFeedContentSuggestions', 'NetworkTimeServiceQuerying', 'AimEnabled', 'AimServerEligibilityEnabled', 'AimServerRequestOnStartupEnabled'
foreach ($proc in Get-CimInstance Win32_Process -Filter "Name = 'Krunk.exe'") {
  $kind = if ($proc.CommandLine -match '--type=(\S+)') { $Matches[1] } else { 'browser' }
  if ($kind -ne 'browser' -and $kind -ne 'gpu-process') { continue }
  $lists = @([regex]::Matches($proc.CommandLine, '--disable-features=(\S+)') | ForEach-Object { $_.Groups[1].Value.Trim('"') })
  $have = ($lists -join ',') -split ','
  $missing = @($wanted | Where-Object { $have -notcontains $_ })
  Log "[$P] $kind process: $($lists.Count) --disable-features switch(es), $($have.Count) names, all 12: $($missing.Count -eq 0) (missing: $($missing -join ', ')): $($lists -join ' | ')"
}
SafeGo 'krunk://gpu' 3000
Click (X 881) (Y 210) 3000                              # Open Chromium's chrome://gpu
Shot "$P-1-chrome-gpu"
# Scroll the Command Line row to the middle of the page.
Wheel (X 640) (Y 400) 3
Start-Sleep 1
Shot "$P-2-command-line"
# The Command Line cell's text: triple-click selects it, Ctrl+C copies it.
[U]::SetCursorPos(700, 405) | Out-Null; Start-Sleep -Milliseconds 200
for ($i = 0; $i -lt 3; $i++) { [U]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [U]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80 }
Set-Clipboard -Value 'empty'; Keys '^c' 800
$row = (Get-Clipboard) -join ' '
$lists = @([regex]::Matches($row, '--disable-features=(\S+)') | ForEach-Object { $_.Groups[1].Value.Trim('"') })
$have = ($lists -join ',') -split ','
$missing = @($wanted | Where-Object { $have -notcontains $_ })
Log "[$P] chrome://gpu Command Line row: $($lists.Count) --disable-features switch(es), $($have.Count) names, all 12: $($missing.Count -eq 0) (missing: $($missing -join ', '))"
Log "[$P] row text: $row"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
