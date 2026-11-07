# Krunk 0.6.0 milestone 12 (R23): payment methods. A fresh profile gets a vault; a checkout
# (www/8094/checkout.html) with a typed card offers "Save this card?" (brand + last four);
# krunk://passwords lists it masked; after a restart (vault locked) the card field's dropdown
# says "Unlock Krunk passwords", the master password unlocks, the card is picked and the
# number, expiry and name are filled while the security code stays empty; a private tab offers
# nothing; the profile holds no plaintext number.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "cards-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060c'
$master = 'Krunk-test-master-1'
$number = '4111 1111 1111 1111'
function StartKrunk([bool]$fresh) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  if ($fresh) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
  New-Item -ItemType Directory -Force $profile | Out-Null
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
  $A
}
$A = StartKrunk $true
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function PopupRectNow {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
function PopupLog([string]$what) { $pr = PopupRectNow; Log "[$P] $what popup: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"; $pr }
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
# (1) The vault (the 0.5 autofill script's coordinates).
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-1-vault"
# (2) A checkout with a typed card: Pay offers to save it (the vault is open after setup).
SafeGo 'http://localhost:8094/checkout.html' 3500
Click (X 170) (Y 237) 300; Keys 'Alice Example' 200
Click (X 170) (Y 315) 300; Keys $number 200
Click (X 170) (Y 393) 300; Keys '12/30' 200
Click (X 170) (Y 471) 300; Keys '123' 200
Shot "$P-2-typed"
Keys '{ENTER}' 2500
$pr = PopupLog 'save offer'
Shot "$P-3-save-offer"
if ($pr) { Click ($pr.Right - 57) ($pr.Top + 103) 2500 }           # Save card (the rightmost button)
# (3) The list, masked.
SafeGo 'krunk://passwords' 3500
Shot "$P-4-listed-masked"
Quit
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk-1.log" -ErrorAction SilentlyContinue   # the log restarts with Krunk
# (4) Locked after a restart: Unlock in the card dropdown, the master prompt, the pick, the fill.
$A = StartKrunk $false
SafeGo 'http://localhost:8094/checkout.html' 3500
Click (X 170) (Y 315) 1500
$pr = PopupLog 'locked dropdown'
Shot "$P-5-locked-unlock-row"
if ($pr) { Click ($pr.Left + 120) ($pr.Top + 24) 1800 }
Shot "$P-6-master-prompt"
Keys $master 200; Keys '{ENTER}' 3000
$pr = PopupLog 'card dropdown'
Shot "$P-7-cards-after-unlock"
if ($pr) { Click ($pr.Left + 120) ($pr.Top + 24) 1500 }
Shot "$P-8-filled-cvc-empty"
# (5) A second checkout with the same card offers nothing (already stored).
Click (X 170) (Y 471) 300; Keys '123' 200; Keys '{ENTER}' 2500
$pr = PopupLog 'second submit'
Shot "$P-9-no-second-offer"
# (6) A private tab: no dropdown.
SafeKeys '^+n' 2500; SafeKeys 'http://localhost:8094/checkout.html' 100; SafeKeys '{ENTER}' 3500
Click (X 170) (Y 315) 1500
$pr = PopupLog 'private'
Shot "$P-10-private-no-dropdown"
Quit
# (7) No plaintext: the whole profile (and the log) searched for the number in every spacing.
$digits = $number -replace ' ', ''
$hits = @(Get-ChildItem $profile -Recurse -File | Select-String -SimpleMatch -Pattern $digits, $number, '4111-1111-1111-1111' -List)
Log "[$P] plaintext search: $($hits.Count) file(s) contain the card number ($(($hits | ForEach-Object { $_.Path }) -join ', '))"
Log "[$P] vault: $((Get-Item "$profile\Krunk\vault.json" -ErrorAction SilentlyContinue).Length) bytes; format $((Get-Content "$profile\Krunk\vault.json" -Raw -ErrorAction SilentlyContinue | Select-String -Pattern '"format":\s*(\d)').Matches[0].Groups[1].Value)"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'card|vault' | Select-Object -Last 12) -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk-2.log" -ErrorAction SilentlyContinue
Log "[$P] done"
