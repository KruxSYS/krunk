# Krunk 0.5.0 milestone 7: the tabbed settings page (every tab, the search, a deep link typed
# as krunk://settings#privacy), the user-agent picker (the default shown equals what a page
# sees; a chosen preset reaches a page and the server), and krunk://gpu. -Theme dark repeats the
# tab screenshots in the dark theme (Windows apps mode); -Phase nogpu starts with hardware
# acceleration off (the profile edited beforehand) and checks krunk://gpu says so. Needs
# serve.ps1 on :8094. Screenshots "set-<theme>-*" / "gpu-nogpu-*".
param([string]$Theme = 'light', [string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = if ($Phase -eq 'nogpu') { 'gpu-nogpu' } else { "set-$Theme" }
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p050t'
if ($Phase -ne 'nogpu') {
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
}
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$lightBefore = (Get-ItemProperty $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $(if ($Theme -eq 'dark') { 0 } else { 1 }) -Type DWord
$mark = Get-Date -Format o
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
# The sidebar tabs: 24 DIP in from the page's left edge, 36 DIP apart from y = 104 + 24 + 80.
$tabX = X 200
$tabY = @{ general = 198; privacy = 235; passwords = 271; downloads = 308; appearance = 344; advanced = 380 }

if ($Phase -eq 'nogpu') {
  SafeGo 'krunk://gpu' 4000
  Shot "$P-1-gpu-page"
  Log "[gpu] processes with --type=gpu-process: $(@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'Krunk.exe' -and $_.CommandLine -like '*--type=gpu-process*' }).Count)"
} else {
  SafeGo 'krunk://settings' 3500
  Shot "$P-1-general"
  foreach ($t in 'privacy', 'passwords', 'downloads', 'appearance', 'advanced') {
    Click $tabX (Y $tabY[$t]) 900
    Shot "$P-2-$t"
  }
  # Search across the tabs.
  Click (X 1000) (Y 128) 400
  Keys 'cookies' 700
  Shot "$P-3-search-cookies"
  Keys '^a{BACKSPACE}' 500
  # A deep link typed in the address bar.
  SafeGo 'krunk://settings#privacy' 3000
  Shot "$P-4-deep-link-privacy"
  if ($Theme -eq 'light') {
    # The user agent: the default shown on the page equals what a page sees.
    SafeGo 'http://localhost:8094/ua.html' 3000
    Shot "$P-5-ua-default-page"
    SafeGo 'krunk://settings#advanced' 3000
    Shot "$P-6-advanced-ua"
    # Identify as Firefox: the picker is the second row's select; keyboard-driven for robustness:
    # click it, then choose by typing the first letters.
    Click (X 998) (Y 400) 600
    Keys 'Firefox 159 on W' 300; Keys '{ENTER}' 1500
    Shot "$P-7-ua-firefox-chosen"
    SafeGo 'http://localhost:8094/ua.html' 3000
    Shot "$P-8-ua-firefox-page"
    SafeGo 'https://httpbin.org/user-agent' 6000
    Shot "$P-9-ua-firefox-httpbin"
    # Back to the default.
    SafeGo 'krunk://settings#advanced' 3000
    Click (X 998) (Y 400) 600
    Keys 'Krunk' 300; Keys '{ENTER}' 1500
    SafeGo 'http://localhost:8094/ua.html' 3000
    Shot "$P-10-ua-default-again"
    # krunk://gpu and Chromium's chrome://gpu from its button.
    SafeGo 'krunk://gpu' 4000
    Shot "$P-11-gpu-page"
    Click (X 880) (Y 215) 5000
    Shot "$P-12-chrome-gpu"
    SafeGo 'krunk://about' 2500
    Shot "$P-13-about"
  }
}
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
if ($null -ne $lightBefore) { Set-ItemProperty $personalize -Name AppsUseLightTheme -Value $lightBefore -Type DWord }
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'browser arguments|user agent|blocked navigation|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line.Substring(0, [Math]::Min(400, $_.Line.Length)))" }
Get-Content 'C:\pipeline\krunk\www\access.log' | Where-Object { $_ -gt $mark -and $_ -match 'log\?ua=' } | ForEach-Object {
  $q = (($_ -split 'log\?ua=')[1] -split ' cookie=')[0]; Log "[ua-seen-by-server] $([uri]::UnescapeDataString($q))"
}
