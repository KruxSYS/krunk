# Krunk 0.6.0 milestone 9 (R21): request headers. Settings → Advanced → Request headers:
# a platform of "Krunk OS" in the client hints, Accept-Language de-DE, a custom header
# X-Krunk-Test: 1 and a rule removing X-Page-Header (which the test page sends itself).
# The echo at https://www.krunk-test.test:8443/headers shows what arrived; the page shows
# navigator.userAgentData.platform and that window.__lbHost does not exist. Then Reset.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "hdr-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
foreach ($port in '8094', '8443') {
  $prefix = if ($port -eq '8443') { 'https://*:8443/' } else { 'http://*:8094/' }
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',$prefix
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p060h'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
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
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1; Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue }
# A settings row found through the search box: the only row shown, at a fixed place
# (measured at 100 % with the General group title above: the row's control at y 340, the
# custom-header form at y 476, y 468 once a rule is listed).
function Field([string]$query) {
  SafeGo 'krunk://gpu' 1500                      # a full load each time: no old filter
  SafeGo 'krunk://settings' 2500                 # no #advanced: the fragment scroll raced the page's own scrollTo(0,0)
  Click (X 968) (Y 146) 300; Keys '^a' 100; Keys $query 800
}
# ---- Before: the engine's own values.
SafeGo 'https://www.krunk-test.test:8443/ua.html' 4500
Shot "$P-1-before"
# ---- Accept-Language
Field 'Accept-Language'
Click (X 1038) (Y 340) 300; Keys 'de-DE, de' 200; Keys '{TAB}' 1500
Shot "$P-2-accept-language"
# ---- Client hints on, platform "Krunk OS".
Field 'Client hints'
# From the search box, Tab passes the seven section links and lands on the switch; after it
# the fields follow in order (brands, full version, platform).
Keys '{TAB}{TAB}{TAB}{TAB}{TAB}{TAB}{TAB}{TAB}' 300; Keys ' ' 1500
Keys '{TAB}' 200; Keys '{TAB}' 200; Keys '{TAB}' 200; Keys '^a' 100; Keys 'Krunk OS' 200; Keys '{TAB}' 1500
Shot "$P-3-hints"
# ---- Custom headers: remove X-Page-Header on krunk-test.test; X-Krunk-Test: 1 everywhere.
Field 'Custom headers'
Click (X 458) (Y 476) 300; Keys 'X-Page-Header' 100; Keys '{TAB}' 100; Keys '{TAB}' 100; Keys 'krunk-test.test' 100; Keys '{ENTER}' 1500
Shot "$P-4a-one-rule"
Field 'Custom headers'
Click (X 458) (Y 468) 300; Keys 'X-Krunk-Test' 100; Keys '{TAB}' 100; Keys '1' 100; Keys '{ENTER}' 1500
Shot "$P-4-rules"
# ---- A new tab gets the override before its first request.
SafeKeys '^t' 1500
SafeGo 'https://www.krunk-test.test:8443/ua.html' 5000
Shot "$P-5-after"
Log "[$P] title: $([TW]::Txt((Frames)[0]))"
# ---- Reset.
Field 'Reset request headers'
Click (X 1090) (Y 341) 1500
Shot "$P-6-reset"
SafeGo 'https://www.krunk-test.test:8443/ua.html' 5000
Shot "$P-7-after-reset"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'request headers|user-agent override|accept_languages') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
Log "[$P] done"
