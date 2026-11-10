# Krunk 0.11.7 audit checks on Windows (fresh profile, test server on 8094, rangeserve on 8093):
#   C4  two extensions side by side: krunk-fixture works (banner, storage, background); the
#       audit's krunk-attacker can't see the fixture's binding, and a forged call naming the
#       fixture gets its own storage back (access.log c4-*).
#   C1  the attacker's background opens krunk://settings and tries scripting.executeScript in
#       it (refused), then in the web tab that asked (runs: c1-web-ran).
#   C3  www/8094/audit.html: window.open of krunk.invalid/source.html?url=… (the source page
#       must not load: no c3-source-fetched), and about:blank then location = settings.
#   C2  the same page downloads a blob named 250 × "a" + ".aaaaaaaaaaaaaaaé": Krunk lives on,
#       the file is saved with a cut name.
#   C7  big.bin (64 MB) from rangeserve: fetched in parts, has a Zone.Identifier stream; the
#       C2 blob (an engine download) too.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'audit'
$dl = "$env:USERPROFILE\Downloads"
$www = 'C:\pipeline\krunk\www'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
# rangeserve for C7 (as in vm-040-parts.ps1).
$rr = "$www\8093"
New-Item -ItemType Directory -Force $rr | Out-Null
if (-not (Test-Path "$rr\big.bin")) { $b = New-Object byte[] (64MB); (New-Object Random 11).NextBytes($b); [IO.File]::WriteAllBytes("$rr\big.bin", $b) }
Copy-Item C:\pipeline\krunk\rangeserve.py "$www\rangeserve.py" -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'python.exe' -and $_.CommandLine -match 'range[s]erve\.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process 'C:\Program Files\Python312\python.exe' -WindowStyle Hidden -ArgumentList "`"$www\rangeserve.py`"", "`"$rr`"", '8093'
Start-Sleep 2
Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
Get-ChildItem $dl -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'aaaa*' -or $_.Name -like 'big*' } | Remove-Item -Force -ErrorAction SilentlyContinue
$profile = 'C:\pipeline\krunk\p0117'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$klog = "$profile\Krunk\krunk.log"
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus A"; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
$page = 104
function Title { [TW]::Txt((MainWindow)) }
# Another program may hold the foreground (the VM is shared): bring Krunk back first.
function Refocus { Focus (MainWindow) | Out-Null; Start-Sleep -Milliseconds 300 }
function Alive { [bool](Get-Process Krunk -ErrorAction SilentlyContinue) }
function Access([string]$pattern) { (Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { ($_.Line -split ' ')[2] }) -join ' | ' }
function KLog([string]$pattern, [int]$n = 4) { (Get-Content $klog -ErrorAction SilentlyContinue | Select-String $pattern | Select-Object -Last $n | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ' }

# (1) Both extensions from their zips (each install asks first).
# (0.11.x layout: "Install from file…" is in the page header, right.)
foreach ($zip in 'krunk-fixture.zip', 'krunk-attacker.zip') {
  Refocus
  SafeGo 'krunk://extensions' 3000
  Click (X 904) (Y 144) 2500
  Keys "C:\pipeline\krunk\vm-ext\$zip" 300; Keys '{ENTER}' 6000
  Keys '{ENTER}' 4000
}
Refocus
SafeGo 'krunk://extensions' 3000
Shot "$P-1-two-extensions"
Log "[$P] installed: $(KLog 'extension.*install' 4)"

# (2) C4 and C1: the test page with both content scripts; the attacker's background opens
# krunk://settings and tries to script it.
Refocus
SafeGo 'http://localhost:8094/ext.html' 9000
Shot "$P-2-after-c1"
Log "[$P] C1 title now: $(Title)"
Refocus; SafeKeys '^w' 1500
Shot "$P-3-fixture-banner"
Log "[$P] C4 access.log: $(Access 'c4-')"
Log "[$P] C1 access.log: $(Access 'c1-')"
Log "[$P] krunk.log ext: $(KLog 'extension|world' 8)"

# (3) C3: the audit page's two popup tries.
Refocus
SafeGo 'http://localhost:8094/audit.html' 3000
Shot "$P-4-audit-page"
Click (X 400) (Y ($page + 50)) 3500
Shot "$P-5-c3a-popup"
Log "[$P] C3a title: $(Title)"
Refocus; SafeKeys '^w' 1500
Click (X 400) (Y ($page + 150)) 3500
Shot "$P-6-c3b-popup"
Log "[$P] C3b title: $(Title)"
Refocus; SafeKeys '^w' 1500
Log "[$P] C3 access.log: clicks [$(Access 'clicked=c3|c3a-|c3b-')] source fetched [$(Access 'c3-source-fetched')]"
Log "[$P] C3 krunk.log: $(KLog 'blocked navigation' 4)"

# (4) C2: the long blob name.
Click (X 400) (Y ($page + 250)) 5000
Shot "$P-7-c2-download"
$f = Get-ChildItem $dl -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'aaaa*' } | Select-Object -First 1
Log "[$P] C2 Krunk alive: $(Alive); file: $(if ($f) { "$($f.Name.Length) chars, ends '$($f.Name.Substring($f.Name.Length - 20))', $($f.Length) bytes" } else { 'none' })"
if ($f) { Log "[$P] C7 engine download Zone.Identifier: $((Get-Content -LiteralPath $f.FullName -Stream Zone.Identifier -ErrorAction SilentlyContinue) -join ' / ')" }

# (5) C7: 64 MB in parts.
$t0 = Get-Date
Refocus
SafeGo 'http://127.0.0.1:8093/big.bin' 1000
for ($i = 0; $i -lt 240; $i++) {
  $big = Get-ChildItem $dl -Filter 'big*.bin' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike '*.part*' -and $_.Length -eq 64MB } | Select-Object -First 1
  if ($big) { break }
  Start-Sleep -Milliseconds 250
}
Start-Sleep 1
Shot "$P-8-c7-parts"
Log "[$P] C7 big.bin: $(if ($big) { "complete after $([math]::Round(((Get-Date) - $t0).TotalSeconds, 1)) s" } else { 'not complete' }); $(KLog 'in parts|parts' 3)"
if ($big) { Log "[$P] C7 parts download Zone.Identifier: $((Get-Content -LiteralPath $big.FullName -Stream Zone.Identifier -ErrorAction SilentlyContinue) -join ' / ')" }
Log "[$P] Krunk alive at the end: $(Alive)"
Copy-Item $klog "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Copy-Item "$www\access.log" "$Shots\$P-access.log" -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-ChildItem $dl -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'aaaa*' -or $_.Name -like 'big*' } | Remove-Item -Force -ErrorAction SilentlyContinue
