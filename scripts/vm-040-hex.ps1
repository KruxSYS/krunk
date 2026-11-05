# Krunk 0.4.0 milestone 9: the hex editor of krunk://sitedata. vm-www/8092/bin.html stores a
# binary string in localStorage (00 01 02 FF FE 41) and a percent-encoded cookie (v1%00%01%FF);
# both are edited in hex, and the page reads back the exact bytes (its title, in run.log).
# Fresh profile. Screenshots "hex-*".
param([switch]$Explore)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'hex'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p040h'
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
$page = [int]($c.Top + 104 * $c.Scale)
$col = [int]($c.Left + ((($c.Right - $c.Left) / $c.Scale - 960) / 2 + 24) * $c.Scale)
function At([int]$x, [int]$y) { @([int]($col + $x * $c.Scale), [int]($page + $y * $c.Scale)) }
function Page([string]$label) {
  SafeKeys '^1' 800; SafeGo 'http://localhost:8092/bin.html' 3000
  Log "[$label] page reads: $([TW]::Txt($A))"
  Shot "$P-$label"
}
SafeGo 'http://localhost:8092/bin.html?reset=1' 3000
Log "[1-start] page reads: $([TW]::Txt($A))"
# Shield, then "Cookies and site data" (the second control of the panel).
Click ([int]($c.Right - 56 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 1500
Keys '{TAB}' 300; Keys '{ENTER}' 3500
Shot "$P-2-sitedata"
if ($Explore) { return }

# localStorage "bin": opens on Hex with the caret on byte 0. Type over it (4B), two right,
# Insert, type 7A, Delete (the FF): 4B 01 02 7A FE 41. Then Tab Tab Enter: Save.
$pt = At 852 284; Click $pt[0] $pt[1] 1500
Shot "$P-3-bin-hex"
Keys '4B' 300; Keys '{RIGHT}{RIGHT}' 300; Keys '{INSERT}' 200; Keys '7A' 300; Keys '{DELETE}' 500
Shot "$P-4-bin-edited"
Keys '{TAB}{TAB}' 300; Keys '{ENTER}' 2000
Shot "$P-5-saved"
# The cookie: v1%00%01%FF is 76 31 00 01 FF; byte 2 becomes 7E: v1~%01%FF.
$pt = At 855 188; Click $pt[0] $pt[1] 1500
Shot "$P-6-cookie-hex"
Keys '{RIGHT}{RIGHT}' 300; Keys '7E' 500
Shot "$P-7-cookie-edited"
Keys '{TAB}{TAB}' 300; Keys '{ENTER}' 2000
Shot "$P-8-saved"
Page '9-page-reads'
KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'site data|cookie|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
