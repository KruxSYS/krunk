# Krunk 0.7.0 M1 (R23): one --disable-features with CEF's four features and Krunk's eight
# (chrome://version, and the GPU process's switches, which the engine builds from the features
# in force); krunk://gpu with the engine line, the real command line and H.264/AAC "yes"; the
# engine file check in krunk.log; a changed engine file is found and reported.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'engine'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = "$K\p070e"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function StartKrunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
  $script:c = ClientRect $A
}
StartKrunk
$wanted = 'GlicActorUi', 'AutofillActorMode', 'LensOverlay', 'KillOnInvalidNavigationHeaders', 'AutofillServerCommunication', 'MediaRouter',
  'OptimizationHints', 'InterestFeedContentSuggestions', 'NetworkTimeServiceQuerying', 'AimEnabled', 'AimServerEligibilityEnabled', 'AimServerRequestOnStartupEnabled'
# The GPU process gets the features the browser process runs with (one switch, rebuilt from them).
Start-Sleep 3
# (PowerShell names are case-insensitive: never $p next to $P.)
foreach ($proc in Get-CimInstance Win32_Process -Filter "Name = 'Krunk.exe'") {
  if ($proc.CommandLine -match '--type=gpu-process') {
    $lists = [regex]::Matches($proc.CommandLine, '--disable-features=(\S+)') | ForEach-Object { $_.Groups[1].Value }
    Log "[$P] gpu process: $($lists.Count) --disable-features switch(es): $($lists -join ' | ')"
    $have = ($lists -join ',') -split ','
    $missing = $wanted | Where-Object { $have -notcontains $_ }
    Log "[$P] gpu process: all 12 features disabled: $($missing.Count -eq 0) (missing: $($missing -join ', '))"
  }
}
SafeGo 'krunk://gpu' 2500
Shot "$P-1-gpu-checking"
Start-Sleep 25
SafeGo 'krunk://about' 1500
SafeGo 'krunk://gpu' 3000
Shot "$P-3-gpu"
Click ([int](($c.Left + $c.Right) / 2)) ($c.Bottom - 40) 300
Keys '{END}' 1500
Shot "$P-4-gpu-codecs"
SafeGo 'krunk://about' 2500
Shot "$P-5-about"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'engine files|release files|engine switch|browser arguments' | ForEach-Object { Log "[$P] log: $_" }
# Chromium's own chrome://gpu through the page's button (typed chrome:// addresses are searched).
SafeGo 'krunk://gpu' 3000
Click (X 881) (Y 210) 3000
Shot "$P-6-chrome-gpu"
Keys '{END}' 1500
Shot "$P-7-chrome-gpu-end"
# A damaged engine file: found at the next start, krunk://gpu says so and offers the repair.
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
$pak = "$K\app\locales\fr.pak"
Copy-Item $pak "$K\fr.pak.bak" -Force
Add-Content -Path $pak -Value 'x' -NoNewline
StartKrunk
Start-Sleep 25
# The check finds the file and the repair prompt opens (portable copy: it offers the download).
Shot "$P-8-repair-prompt"
Keys '{ESC}' 800
Focus $A | Out-Null
SafeGo 'krunk://gpu' 3000
Shot "$P-9-gpu-damaged"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'engine files' | ForEach-Object { Log "[$P] log (damaged): $_" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
Copy-Item "$K\fr.pak.bak" $pak -Force
Remove-Item "$K\fr.pak.bak"
Log "[$P] fr.pak restored: $((Get-FileHash $pak).Hash.Substring(0,16))"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
