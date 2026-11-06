# Krunk 0.5.0 milestone 6: the lock popup and the restyled shield, in the light and the dark
# theme. The site icon opens the site-info popup for an https site (example.com), an http site
# (neverssl.com), expired and self-signed badssl.com sites and mixed content; "Certificate
# details" expands the chain. The shield popup is shown on a page with blocked requests
# (vm-www/8092/ads.html, needs serve.ps1 on :8092). Fresh profile. Screenshots "si-<theme>-*".
param([string]$Theme = 'light')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "si-$Theme"
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p050i'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
# Krunk's default theme follows Windows: dark apps mode gives the dark theme (shell, popups, pages).
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$lightBefore = (Get-ItemProperty $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $(if ($Theme -eq 'dark') { 0 } else { 1 }) -Type DWord
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$iconX = X 133; $toolbarY = Y 56
function SiteInfo([string]$url, [string]$name, [int]$wait) {
  SafeGo $url $wait
  Click $iconX $toolbarY 1500
  Shot "$P-$name"
  Keys '{ESC}' 600
}
SiteInfo 'https://example.com/' '1-https' 4000
# "Certificate details" expands the chain: the link sits under the leaf card; click it by its
# position (about 240 DIP below the popup's top at 100 %), then screenshot.
Click $iconX $toolbarY 1500
Click (X 190) (Y (56 + 16 + 236)) 1200
Shot "$P-2-https-details"
Keys '{ESC}' 600
SiteInfo 'http://neverssl.com/' '3-http' 5000
SiteInfo 'https://expired.badssl.com/' '4-expired' 6000
SiteInfo 'https://self-signed.badssl.com/' '5-self-signed' 6000
SiteInfo 'https://mixed.badssl.com/' '6-mixed' 7000
SiteInfo 'krunk://settings' '7-internal' 2500
# The shield on a page with blocked requests.
SafeGo 'http://localhost:8092/ads.html' 4000
Click ([int]($c.Right - 56 * $c.Scale)) $toolbarY 1500
Shot "$P-8-shield"
Keys '{ESC}' 600
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
if ($null -ne $lightBefore) { Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $lightBefore -Type DWord }
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | ForEach-Object { Log "[log] $_" }
