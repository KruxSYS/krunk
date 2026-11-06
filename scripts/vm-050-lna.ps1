# Krunk 0.5.0 milestone 10: Local Network Access. From https://example.com (a public page) the
# DevTools console fetches http://localhost:8094 (this computer, "loopback") and the VM's LAN
# address. Before 0.5.0 those requests waited forever (M1). Now Krunk's prompt appears:
#   Allow + remember → the fetch succeeds (title "LNA-…-OK 200"), a reload asks nothing;
#   krunk://sitedata shows "Local network access: Allowed" with Remove;
#   after Remove the prompt comes back; Block → the fetch fails ("LNA-…-FAIL …");
#   a private tab asks every time (nothing remembered).
# The page reports through document.title (read from the window title). Screenshots "lna-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'lna'
foreach ($port in 8094) {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match "krunk\\[s]erve\.ps1.*$port" })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList',"http://*:$port/"
    Start-Sleep 2
  }
}
$profile = 'C:\pipeline\krunk\p050lna'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$lan = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.PrefixOrigin -ne 'WellKnown' } | Select-Object -First 1).IPAddress
Log "[setup] LAN address $lan"

function Title { [TW]::Txt($A) }
# Types a console snippet (SendKeys escapes: ( ) { } + ^ % ~ [ ] are typed as {(} {)} {{} {}} {+} ...).
function Snippet([string]$js, [int]$wait = 3000) { Keys $js 300; Keys '{ENTER}' $wait }
function FetchLoopback([string]$tag) { Snippet "fetch{(}'http://localhost:8094/log?lna=$tag'{)}.then{(}r=>document.title='LNA-LOOPBACK-OK '{+}r.status,e=>document.title='LNA-LOOPBACK-FAIL '{+}e.message{)}" }
function FetchLan([string]$tag) { Snippet "fetch{(}'http://$($lan):8094/log?lna=$tag'{)}.then{(}r=>document.title='LNA-LAN-OK '{+}r.status,e=>document.title='LNA-LAN-FAIL '{+}e.message{)}" }
# DevTools stays open across navigations and F12 only toggles it: close it first (the strip's
# × at the top right; a click there is harmless when DevTools is closed), then open the console.
# DevTools stays open across navigations and F12 only toggles it: close it first (the strip's
# × at the top right; a click there is harmless when DevTools is closed), open it, pick its
# Console tab and click into the console so the typed snippet lands in the prompt (Esc would
# toggle the drawer, and a '?' typed outside the prompt opens DevTools' settings).
function OpenConsole { Click 1258 116 800; SafeGo 'https://example.com/' 4000; SafeKeys '{F12}' 4000; Click 930 142 1000; Click 1000 660 500 }

try {
# (1) Loopback from a public page: the prompt, Allow (remembered).
OpenConsole
FetchLoopback 'first'
Shot "$P-1-prompt-loopback"
Log "[1] title while the prompt is up: $(Title)"
Click 1218 204 2500   # Allow (the prompt's primary button, bottom right)
Log "[2] after Allow: $(Title)"
Shot "$P-2-allowed-fetch-ok"
# (2) A reload asks nothing: the stored choice answers.
OpenConsole
FetchLoopback 'second'
Log "[3] after reload, no prompt: $(Title)"
Shot "$P-3-remembered-no-prompt"
# (3) The LAN address is a separate request (local network, not loopback), same site: the
#     stored choice covers it too.
FetchLan 'lan-first'
Log "[4] LAN with the stored choice: $(Title)"
Shot "$P-4-lan-with-stored-choice"
Click 1258 116 1000
# (4) krunk://sitedata shows and removes the stored permission (shield panel → Cookies and site data).
Click 1215 56 1500
Shot "$P-5-shield-panel"
Click 1140 214 3500   # "Cookies and site data" in the shield panel
Shot "$P-6-sitedata-allowed"
Click 1054 266 2000   # Remove
Shot "$P-7-sitedata-removed"
SafeKeys '^w' 1500    # the site-data tab closes; back to the example.com tab
# (5) The prompt is back; Block (remembered) makes the fetch fail; sitedata says Blocked.
OpenConsole
FetchLoopback 'after-remove'
Shot "$P-8-prompt-again"
Click 1153 204 2500   # Block
Log "[5] after Block: $(Title)"
Shot "$P-9-blocked-fetch-fails"
OpenConsole
FetchLoopback 'blocked-stored'
Log "[6] blocked, stored: $(Title)"
Click 1258 116 1000
Click 1215 56 1500
Click 1140 214 3500
Shot "$P-10-sitedata-blocked"
SafeKeys '^w' 1500
# (6) Private tab: the prompt every time, nothing remembered.
SafeKeys '^+n' 2500
OpenConsole
FetchLoopback 'private-first'
Shot "$P-11-private-prompt"
Click 1218 204 2500
Log "[7] private after Allow: $(Title)"
OpenConsole
FetchLoopback 'private-second'
Shot "$P-12-private-asks-again"
Log "[8] private, second time (prompt expected): $(Title)"
Keys '{ESC}' 1500
Log "[9] private after Esc (blocked once): $(Title)"
} catch { Log "[error] $_ at line $($_.InvocationInfo.ScriptLineNumber)" }
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'local network|permission|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line)" }
Get-Content C:\pipeline\krunk\www\access.log | Select-String 'lna=' | ForEach-Object { Log "[access] $($_.Line)" }
