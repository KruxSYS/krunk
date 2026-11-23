# Krunk 0.8.0 (C, items 16-17): Settings → Account. $Phase:
#   release  the release build: an idle start with Settings → Account open (signed out) under a
#            pktmon capture of port 443: no packet to krunk.kruxsys.com's addresses. Then Sign in once:
#            live has no link endpoints before S3 → the card says sign-in isn't available (404).
#   offline  the --dev build (site http://127.0.0.1:18080) with the tunnel down: Sign in → "couldn't
#            reach".
#   staging  the --dev build with the tunnel to the staging stack up: 30 s idle (the staging api
#            log must show no /api request then), Sign in, the code read from the opened tab's
#            address, approved from Linux (it waits for C:\pipeline\krunk\approved), the card
#            signed in, account.json DPAPI-wrapped, the ⋮ label, Sign out → account.json gone.
# Updates are switched off in the test profiles (the update check would also reach the site).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'acct080'
$K = 'C:\pipeline\krunk'
$Phase = 'release'
$App = if ($Phase -eq 'release') { "$K\app" } else { "$K\appdev" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item "$K\approved", "$K\code.txt" -Force -ErrorAction SilentlyContinue
$profile = "$K\p080c-$Phase"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
Set-Content -Encoding ascii "$K\patch.json" '{"checkUpdatesAutomatically":false}'
Log "[$P] $Phase settings: $(python $K\setkv.py $profile "@$K\patch.json")"
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
  $script:c = ClientRect $A
}
# A button of the Account card (CSS px of the page at this window size; measured with Playwright).
# The page starts under the shell (76 DIP) and the bookmarks bar (28 DIP; the template shows it).
function CardClick([int]$x, [int]$y, [int]$wait = 1500) { Click ([int]($c.Left + $x * $c.Scale)) ([int]($c.Top + (104 + $y) * $c.Scale)) $wait }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }
function Finish {
  if (Focus $A) { SafeKeys '%{F4}' 2500 }
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  Log "[$P] $Phase account lines: $(KLog 'account:')"
  Log "[$P] $Phase WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
  Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-$Phase-krunk.log"
}

if ($Phase -eq 'release') {
  $ips = @(Resolve-DnsName krunk.kruxsys.com -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress } | ForEach-Object { $_.IPAddress })
  Log "[$P] krunk.kruxsys.com resolves to $($ips -join ', ')"
  pktmon filter remove | Out-Null
  pktmon filter add -p 443 | Out-Null
  pktmon start --capture --pkt-size 0 -f "$Shots\$P-idle.etl" | Out-Null
  $t0 = Get-Date
  StartKrunk
  SafeGo 'krunk://settings#account' 3000
  Shot "$P-release-1-card-idle"
  while (((Get-Date) - $t0).TotalSeconds -lt 75) { Start-Sleep 2 }
  $t1 = Get-Date
  # Sign in, still captured (the positive control): live has no link endpoints yet (404).
  CardClick 1016 208 5000
  Shot "$P-release-2-not-available"
  pktmon stop | Out-Null
  pktmon format "$Shots\$P-idle.etl" -o "$Shots\$P-idle.txt" | Out-Null
  $lines = Get-Content "$Shots\$P-idle.txt" -ErrorAction SilentlyContinue
  # pktmon's text: a header line with the time, then the packet line with the addresses.
  $idle = 0; $after = 0; $all = 0; $ts = $null
  foreach ($l in $lines) {
    $m = [regex]::Match($l, '::(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{1,7})')
    if ($m.Success) { $ts = [datetime]::Parse($m.Groups[1].Value); continue }
    if (-not $ts -or $l -notmatch '\.443[ :]') { continue }
    $all++
    if (-not ($ips | Where-Object { $l -match ([regex]::Escape($_) + '\.443') })) { continue }
    if ($ts -lt $t1) { $idle++ } else { $after++ }
  }
  Log "[$P] idle 75 s with Settings > Account open: $idle packets to krunk.kruxsys.com; after Sign in: $after (the capture works); $all packet lines on port 443 in total"
  Remove-Item "$Shots\$P-idle.etl" -Force -ErrorAction SilentlyContinue
  Log "[$P] release sign-in against live: $(KLog 'account:')"
  Finish
} elseif ($Phase -eq 'offline') {
  StartKrunk
  SafeGo 'krunk://settings#account' 3000
  CardClick 1016 208 6000
  Shot "$P-offline-1-unreachable"
  Finish
} else {
  StartKrunk
  SafeGo 'krunk://settings#account' 3000
  Log "[$P] staging idle start $(Get-Date -Format o)"
  Start-Sleep 30
  Log "[$P] staging idle end $(Get-Date -Format o)"
  Shot "$P-staging-1-signed-out"
  CardClick 1016 208 5000
  # The sign-in page opened in a new tab: its address holds the code.
  SafeKeys '^l' 300; SafeKeys '^c' 300
  $addr = Get-Clipboard
  $code = ([regex]::Match($addr, 'code=([0-9A-Z-]{9})')).Groups[1].Value
  Log "[$P] opened tab: $($addr -replace 'code=.*', 'code=<code>') (code read: $([bool]$code))"
  Set-Content -Encoding ascii "$K\code.txt" $code
  SafeKeys '{ESC}' 200; SafeKeys '^1' 1200
  Shot "$P-staging-2-code"
  for ($i = 0; $i -lt 120 -and -not (Test-Path "$K\approved"); $i++) { Start-Sleep 1 }
  Log "[$P] approved flag after $i s"
  Start-Sleep 12
  Shot "$P-staging-3-signed-in"
  $file = "$profile\Krunk\account.json"
  if (Test-Path $file) {
    $j = Get-Content $file -Raw | ConvertFrom-Json
    Log "[$P] account.json: fields $(($j.PSObject.Properties.Name) -join ','); refresh is DPAPI: $($j.refresh.StartsWith('AQAAANCMnd8BFdERjHoAwE/Cl+s')); code in it: $((Get-Content $file -Raw).Contains($code))"
  } else { Log "[$P] account.json missing" }
  Click ([int]($c.Right - 20 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 1200
  Shot "$P-staging-4-menu"
  Keys '{ESC}' 600
  CardClick 1016 228 6000
  Shot "$P-staging-5-signed-out"
  Log "[$P] after sign-out: account.json exists: $(Test-Path $file)"
  Finish
  Log "[$P] code in krunk.log: $((Get-Content "$profile\Krunk\krunk.log" -Raw).Contains($code))"
}
Log "[$P] done"
