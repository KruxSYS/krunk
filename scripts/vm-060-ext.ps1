# Krunk 0.6.0 milestone 15 (R26): extensions. A fresh profile: krunk://extensions is empty; the
# fixture zip (scripts/vm-ext/krunk-fixture.zip, copied to C:\pipeline\krunk\vm-ext) is
# installed from the file dialog; its toolbar button appears; www/8094/ext.html gets the content
# script's banner (the page sees nothing of the extension) and the rule blocks an image; the
# popup shows the tab and the counters and pings the content script; Options opens in a tab;
# krunk://newtab stays untouched; a private tab gets nothing; disabling removes the button and
# the banner; then two real Web Store extensions are installed by address.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "ext-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060x'
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
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
function LogExt([string]$what) { Log "[$P] $what`: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'extension' | Select-Object -Last 6 | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ')" }
# (1) Empty page; install the fixture from the file dialog.
SafeGo 'krunk://extensions' 3000
Shot "$P-1-empty"
Click (X 236) (Y 207) 2500                                                    # Install from file…
Keys 'C:\pipeline\krunk\vm-ext\krunk-fixture.zip' 300; Keys '{ENTER}' 6000
SafeGo 'krunk://extensions' 3000
Shot "$P-2-installed"
LogExt 'after install'
# (2) The test page: the banner, the blocked image, nothing visible to the page.
SafeGo 'http://localhost:8094/ext.html' 4000
Shot "$P-3-content-script"
# (3) The toolbar button opens the popup (the rightmost "tb" before downloads/shield: find it
# by the KrunkPopup-less window list: the popup is its own top-level window).
$before = @([TW]::All() | Where-Object { (KrunkPids) -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) }).Count
Click (X 1174) (Y 56) 3000                                                    # the extension's button (left of the shield)
LogExt 'popup click'
$after = @([TW]::All() | Where-Object { (KrunkPids) -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) })
$pw = $after | Where-Object { [TW]::Txt($_) -eq 'Krunk popup' -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
Log "[$P] popup window: $(if ($pw) { $r = WindowRect $pw; "$($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { 'none' }) (visible windows $before -> $($after.Count))"
Shot "$P-4-popup"
if ($pw) { $pw2 = [TW]::All() | Where-Object { (KrunkPids) -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Txt($_) -eq 'Krunk popup' } | Select-Object -First 1; $r = WindowRect $pw2; Log "[$P] popup sized: $($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)"; Click ($r.Left + 86) ($r.Top + 241) 1500 }  # Ping the content script
Shot "$P-5-popup-ping"
LogExt 'after ping'
if ($pw) { Click ($r.Left + 194) ($r.Top + 241) 2500 }                      # Options
Shot "$P-6-options-tab"
# (4) Isolation: krunk://newtab and a private tab get nothing.
SafeGo 'krunk://newtab' 2500
Shot "$P-7-newtab-untouched"
# (5) Disable: the button and the banner go; enable again.
SafeGo 'krunk://extensions' 3000
Keys '{F5}' 2500
Click (X 278) (Y 512) 1500                                                    # Enabled checkbox of the first card
Shot "$P-9-disabled"
SafeGo 'http://localhost:8094/ext.html' 3500
Shot "$P-10-disabled-page"
SafeGo 'krunk://extensions' 3000
Keys '{F5}' 2500
Click (X 278) (Y 512) 1500
Shot "$P-9b-enabled-again"
# (6) Two Web Store extensions by address.
Click (X 682) (Y 241) 300; Keys 'https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh' 200; Keys '{ENTER}' 25000
Shot "$P-11-ubo-lite"
Click (X 682) (Y 241) 300; Keys '^a' 100; Keys '{DEL}' 100; Keys 'https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh' 200; Keys '{ENTER}' 25000
Shot "$P-12-dark-reader"
LogExt 'after the store installs'
SafeGo 'https://en.wikipedia.org/wiki/Web_browser' 8000
Shot "$P-13-wikipedia-with-extensions"
Click (X 1174) (Y 56) 4000                                                    # Dark Reader's button (the rightmost: installed last)
LogExt 'store popup click'
Shot "$P-14-store-popup"
# (7) A private tab: no content script (the extension is not allowed there); the page in a private window.
SafeKeys '^+n' 2500; SafeKeys 'http://localhost:8094/ext.html' 100; SafeKeys '{ENTER}' 3500
Shot "$P-8-private-no-script"
SafeGo 'krunk://extensions' 3000
Shot "$P-8b-private-extensions-page"
Log "[$P] extensions dir: $((Get-ChildItem "$profile\Krunk\extensions" -Name) -join ', ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 10 | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ')"
Quit
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
