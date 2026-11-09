# Krunk 0.9.0 (M12): a profile written by 0.8.0 opened by 0.9.0, and sync's opt-in proven on
# the real build. Both builds are --dev builds that talk to the staging stack through the ssh
# tunnel (http://127.0.0.1:18080); the Linux side approves the account link (code.txt →
# approved) and reads the staging api log for the idle window.
#   prep080  0.8.0 (appdev): sp-tpl + the 050-autofill vault (4 logins), a card saved at a
#            checkout, two pages visited and bookmarked, signed in to the staging account;
#            counts written to p090u-counts-080.txt
#   upgrade  0.9.0 (appdev090) on that profile: browser.db v5 → v6 with every count equal, the
#            vault opens unchanged (format kept), browsing works; then idle with Settings →
#            Account/Sync open, signed in, sync off, until 10 minutes after the start, under a
#            pktmon capture of port 443 (krunk.209.wf); the DPAPI test binary runs meanwhile.
#            Positive control at the end: Turn on sync… → krunk://sync → Turn on (the first
#            sync request); the page is left at the setup step for 090-sync.
param([string]$Phase = 'prep080')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "up090-$Phase"
$K = 'C:\pipeline\krunk'
$profile = "$K\p090u"
$master = 'Krunk-test-master-1'
$App = if ($Phase -eq 'prep080') { "$K\appdev" } else { "$K\appdev090" }
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
  $script:c = ClientRect $A
}
# Page CSS px → screen (the page starts under the shell, 76 DIP, and the bookmarks bar, 28 DIP).
function PageClick([int]$x, [int]$y, [int]$wait = 1500) { Click ([int]($c.Left + $x * $c.Scale)) ([int]($c.Top + (104 + $y) * $c.Scale)) $wait }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }
function Counts {
  python -c @"
import json, sqlite3, sys, os
p = sys.argv[1]
d = sqlite3.connect('file:' + p + '/Krunk/browser.db?mode=ro', uri=True)
q = lambda s: d.execute(s).fetchone()[0]
v = json.load(open(p + '/Krunk/vault.json'))
print('user_version', q('PRAGMA user_version'), '| bookmarks', q('SELECT count(*) FROM bookmarks'), '| urls', q('SELECT count(*) FROM urls'),
      '| visits', q('SELECT count(*) FROM visits'), '| tiles', q('SELECT count(*) FROM tiles'), '| never_save', q('SELECT count(*) FROM vault_never'),
      '| vault format', v.get('format'), '| sealed entries', len(v.get('entries') or []), '| cards', len(v.get('cards') or []),
      '| sync part', 'sync' in v, '| account.json', os.path.exists(p + '/Krunk/account.json'), '| sync.json', os.path.exists(p + '/Krunk/sync.json'))
"@ $profile
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}

if ($Phase -eq 'prep080') {
  Remove-Item "$K\approved", "$K\code.txt" -Force -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  Copy-Item -Recurse "$K\sp-tpl" $profile
  Copy-Item "$K\p050a\Krunk\vault.json" "$profile\Krunk\vault.json"
  Set-Content -Encoding ascii "$K\patch.json" '{"checkUpdatesAutomatically":false}'
  Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
  Log "[$P] 0.8.0 build: $((Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion)"
  StartKrunk
  # The vault (made by the 050-autofill flow): unlock, then save a card at a checkout.
  SafeGo 'krunk://passwords' 3000
  TypeSecret $master; Keys '{ENTER}' 3500
  Shot "$P-1-vault-open"
  SafeGo 'http://localhost:8094/checkout.html' 3500
  Click ([int]($c.Left + 170 * $c.Scale)) ([int]($c.Top + 237 * $c.Scale)) 300; Keys 'Alice Example' 200
  Click ([int]($c.Left + 170 * $c.Scale)) ([int]($c.Top + 315 * $c.Scale)) 300; Keys '4111 1111 1111 1111' 200
  Click ([int]($c.Left + 170 * $c.Scale)) ([int]($c.Top + 393 * $c.Scale)) 300; Keys '12/30' 200
  Click ([int]($c.Left + 170 * $c.Scale)) ([int]($c.Top + 471 * $c.Scale)) 300; Keys '123' 200
  Keys '{ENTER}' 2500
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { $r = WindowRect $pw; Click ($r.Right - 57) ($r.Top + 103) 2500; Log "[$P] card saved through the offer" } else { Log "[$P] no save offer" }
  # History and two bookmarks on the bar.
  SafeGo 'https://example.org/' 3000; SafeKeys '^d' 1000
  SafeGo 'http://localhost:8094/cookie-frame.html' 2500; SafeKeys '^d' 1000
  Shot "$P-2-bookmarked"
  # Sign in to the staging account (0.8.0's card: Sign in at 1016,208 of the page).
  SafeGo 'krunk://settings#account' 3000
  PageClick 1016 208 5000
  # The card's Copy code (the link page opens in a background tab).
  SafeKeys '^1' 1200                                       # Settings (the link page may be in front)
  Set-Clipboard -Value 'empty'
  PageClick 1024 199 800
  $code = ([regex]::Match(((Get-Clipboard) -join ' '), '\b([0-9A-Z]{4}-[0-9A-Z]{4})\b')).Groups[1].Value
  Set-Content -Encoding ascii "$K\code.txt" $code
  Log "[$P] link code read: $([bool]$code)"
  for ($i = 0; $i -lt 120 -and -not (Test-Path "$K\approved"); $i++) { Start-Sleep 1 }
  Log "[$P] approved after $i s"
  Start-Sleep 10
  Shot "$P-3-signed-in"
  Log "[$P] account: $(KLog 'account:')"
  if (Focus $A) { SafeKeys '%{F4}' 3000 }
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  $counts = Counts
  Set-Content "$K\p090u-counts-080.txt" $counts
  Log "[$P] written by 0.8.0: $counts"
  Log "[$P] WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count)"
  Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
} else {
  $before = Get-Content "$K\p090u-counts-080.txt"
  $ips = @(Resolve-DnsName krunk.209.wf -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress } | ForEach-Object { $_.IPAddress })
  Log "[$P] krunk.209.wf resolves to $($ips -join ', ')"
  pktmon filter remove | Out-Null
  pktmon filter add -p 443 | Out-Null
  pktmon start --capture --pkt-size 0 -f "$Shots\$P-idle.etl" | Out-Null
  $t0 = Get-Date
  Log "[$P] window start $(Get-Date $t0 -Format o)"
  StartKrunk
  Log "[$P] 0.9.0 build: $((Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion)"
  Start-Sleep 3
  Log "[$P] storage: $(KLog 'storage|migrat|schema')"
  SafeGo 'http://localhost:8094/cookie-frame.html' 2500
  SafeGo 'krunk://passwords' 3000
  TypeSecret $master; Keys '{ENTER}' 3500
  Shot "$P-1-vault-0.9.0"
  Log "[$P] vault: $(KLog 'vault')"
  SafeGo 'krunk://settings#account' 3500
  Shot "$P-2-settings-account-sync-off"
  # The DPAPI round trip of sync.json's wrapper on this machine (built from the same commit).
  if (Test-Path "$App\Krunk-tests.exe") {
    $out = & "$App\Krunk-tests.exe" sync_tests 2>&1 | Out-String
    Log "[$P] DPAPI test: $(($out -split "`n" | Select-String 'test |test result') -join ' ## ')"
  } else { Log "[$P] no Krunk-tests.exe" }
  # Idle, signed in, sync off, Settings open: until 10 minutes after the start.
  while (((Get-Date) - $t0).TotalSeconds -lt 600) { Start-Sleep 5 }
  $t1 = Get-Date
  Log "[$P] idle end / positive control start $(Get-Date $t1 -Format o)"
  Shot "$P-3-after-idle"
  # Positive control: Turn on sync… (Settings card) opens krunk://sync; Turn on asks the server.
  PageClick 445 471 3000
  PageClick 329 303 6000
  Shot "$P-4-turned-on-setup"
  $t2 = Get-Date
  Log "[$P] positive control end $(Get-Date $t2 -Format o)"
  pktmon stop | Out-Null
  pktmon format "$Shots\$P-idle.etl" -o "$Shots\$P-idle.txt" | Out-Null
  $idle = 0; $after = 0; $all = 0; $ts = $null
  foreach ($l in (Get-Content "$Shots\$P-idle.txt" -ErrorAction SilentlyContinue)) {
    $m = [regex]::Match($l, '::(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{1,7})')
    if ($m.Success) { $ts = [datetime]::Parse($m.Groups[1].Value); continue }
    if (-not $ts -or $l -notmatch '\.443[ :]') { continue }
    $all++
    if (-not ($ips | Where-Object { $l -match ([regex]::Escape($_) + '\.443') })) { continue }
    if ($ts -lt $t1) { $idle++ } else { $after++ }
  }
  Log "[$P] port 443 to krunk.209.wf: $idle packets in the 10-minute window, $after after; $all packet lines on 443 in total (filter lists etc.)"
  Remove-Item "$Shots\$P-idle.etl" -Force -ErrorAction SilentlyContinue
  Log "[$P] counts before (0.8.0): $before"
  Log "[$P] counts after  (0.9.0): $(Counts)"
  Log "[$P] sync lines: $(KLog 'sync:')"
  Log "[$P] WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
  Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
}
Log "[$P] done"
