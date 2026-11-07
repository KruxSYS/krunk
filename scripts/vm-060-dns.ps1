# Krunk 0.6.0 milestone 10 (R20): custom DNS. Settings → Privacy → DNS: Custom with the
# Quad9 DoT preset, then the Cloudflare DoH stamp and plain 8.8.8.8 behind it; pages load in
# normal and private tabs; the Test button; the log shows the stub and the resolver lines;
# a packet capture (pktmon, when it may run) or netstat shows no port-53 traffic from Krunk.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "dns-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p060d'
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
function Field([string]$query) {
  SafeGo 'krunk://gpu' 1500
  SafeGo 'krunk://settings' 2500
  Click (X 968) (Y 146) 300; Keys '^a' 100; Keys $query 800
}
# ---- Settings → DNS: the list first (Custom needs one), then the mode. The address field
# keeps the focus after Enter (the list re-renders, the form stays), so three entries go in
# one after the other: DoT to 1.1.1.1 by address, Cloudflare's DoH stamp, plain 8.8.8.8.
Field 'Resolvers'
Shot "$P-1-dns-card"
Click (X 808) (Y 617) 300
Keys 'tls://1.1.1.1' 100; Keys '{ENTER}' 1500
Keys 'sdns://AgcAAAAAAAAABzEuMS4xLjEAEmRucy5jbG91ZGZsYXJlLmNvbQovZG5zLXF1ZXJ5' 100; Keys '{ENTER}' 1500
Keys '8.8.8.8' 100; Keys '{ENTER}' 1500
Shot "$P-2-three-resolvers"
Field 'Name resolution'
Click (X 392) (Y 418) 2500                                                           # Custom
Shot "$P-3-custom-on"
Field 'Resolvers'
Shot "$P-4-status"
Click (X 969) (Y 572) 3000                                                           # Test (first resolver; the status line sits above the rows)
Click (X 969) (Y 617) 3000                                                           # Test (second)
Shot "$P-5-test"
# Packet capture of port 53 from here on (pktmon runs elevated in this task).
$pktmon = $false
try { & pktmon filter remove 2>&1 | Out-Null; & pktmon filter add -p 53 2>&1 | Out-Null; & pktmon start --capture --pkt-size 0 -f C:\pipeline\krunk\shots\dns.etl 2>&1 | Out-Null; $pktmon = ($LASTEXITCODE -eq 0) } catch { Log "[$P] pktmon unavailable: $_" }
# ---- Pages, normal and private.
SafeGo 'https://example.com/' 5000
Shot "$P-6-page"
SafeKeys '^t' 1000; SafeGo 'https://www.wikipedia.org/' 7000
Shot "$P-7-page2"
SafeKeys '^+n' 2500; SafeKeys 'https://www.cloudflare.com/' 100; SafeKeys '{ENTER}' 7000
Shot "$P-8-private"
Field 'Resolvers'
Shot "$P-9-counters"
Log "[$P] netstat 853/443 to 9.9.9.9 and 1.1.1.1: $((netstat -n | Select-String '9\.9\.9\.9:853|1\.1\.1\.1:443|8\.8\.8\.8:53') -join ' | ')"
Log "[$P] netstat port 53 (any): $((netstat -n | Select-String ':53\s') -join ' | ')"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'dns:') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
if ($pktmon) {
  & pktmon stop 2>&1 | Out-Null
  & pktmon format C:\pipeline\krunk\shots\dns.etl -o C:\pipeline\krunk\shots\$P-port53.txt 2>&1 | Out-Null
  Remove-Item C:\pipeline\krunk\shots\dns.etl -ErrorAction SilentlyContinue
  Log "[$P] pktmon port-53 packets: $(@(Get-Content C:\pipeline\krunk\shots\$P-port53.txt -ErrorAction SilentlyContinue | Select-String 'UDP|TCP' | Where-Object { $_ -notmatch 'PktMon\]' }).Count) (lines: $(@(Get-Content C:\pipeline\krunk\shots\$P-port53.txt -ErrorAction SilentlyContinue).Count))"
}
Log "[$P] done"
