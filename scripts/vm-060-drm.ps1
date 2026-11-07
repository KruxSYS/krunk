# Krunk 0.6.0 milestone 14 (R25): protected content (Widevine). A fresh profile has no CDM and
# the setting off (krunk://gpu says "not installed"); Settings → Privacy & security → Protected
# content on, restart: the engine's component updater downloads the CDM into the profile (the
# script waits for widevinecdm.dll); www/8094/drm.html (Shaka Player, the Angel One Widevine
# test stream, a public licence proxy) plays; krunk://gpu and Settings show the version. Then
# the home pages of Netflix and Spotify load (no accounts here: playback is not attempted;
# without VMP those services refuse an unverified player).
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "drm-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060w'
function StartKrunk([bool]$fresh) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  if ($fresh) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
  $A
}
function CdmDll { Get-ChildItem -Recurse -Filter 'widevinecdm.dll' "$profile\Krunk\profile\WidevineCdm" -ErrorAction SilentlyContinue | Select-Object -First 1 }
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
$A = StartKrunk $true
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# (1) Off by default: no CDM, krunk://gpu says so, the probe page finds no Widevine.
SafeGo 'krunk://gpu' 3000
Keys '{END}' 800                                                               # the engine table is at the bottom
Shot "$P-1-gpu-no-cdm"
SafeGo 'http://localhost:8094/drm.html' 8000
Shot "$P-2-probe-off"
Log "[$P] CDM before: $(if (CdmDll) { (CdmDll).FullName } else { 'none' })"
# (2) The setting on (a filtered row with a long description), restart.
SafeGo 'krunk://settings' 3000
Click (X 968) (Y 146) 300; Keys 'Protected content' 1200
Shot "$P-3-setting"
Click (X 1098) (Y 359) 1500                                                   # the row's checkbox (four-line description + status line)
Shot "$P-4-setting-on"
Quit
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk-1.log" -ErrorAction SilentlyContinue
$A = StartKrunk $false
$c = ClientRect $A
for ($i = 0; $i -lt 36 -and -not (CdmDll); $i++) { Start-Sleep 5 }
Log "[$P] CDM after restart: $(if (CdmDll) { "$((CdmDll).FullName) after $(5 * $i) s" } else { 'none after 180 s' })"
# (3) The test stream plays; the pages show the version.
SafeGo 'http://localhost:8094/drm.html' 20000
Shot "$P-5-shaka-playing"
Start-Sleep 8
Shot "$P-6-shaka-playing-later"
SafeGo 'krunk://gpu' 3000
Keys '{END}' 800
Shot "$P-7-gpu-cdm"
SafeGo 'krunk://settings' 3000
Click (X 968) (Y 146) 300; Keys 'Protected content' 1200
Shot "$P-8-setting-status"
# (4) Services that enforce VMP: the home pages (no accounts; playback not attempted).
SafeGo 'https://www.netflix.com/' 8000
Shot "$P-9-netflix-home"
SafeGo 'https://open.spotify.com/' 8000
Shot "$P-10-spotify-home"
Log "[$P] access log: $((Get-Content C:\pipeline\krunk\www\access.log -ErrorAction SilentlyContinue | Select-String 'drm=' | Select-Object -Last 8 | ForEach-Object { $_.Line -replace '.*drm=', '' }) -join ' ## ')"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'component|widevine|cdm|protected' | Select-Object -Last 8) -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Log "[$P] profile folders: $((Get-ChildItem "$profile\Krunk\profile" -Directory | Select-Object -ExpandProperty Name) -join ',')"
Quit
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk-2.log" -ErrorAction SilentlyContinue
Log "[$P] done"
