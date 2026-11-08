# Krunk 0.7.0 verification: a vault made by the published 0.6.0 survives the in-app update to the
# live 0.7.0, takes a recovery phrase (format 3), and recovery replaces its 0.6.0 password.
# 0.7.1: $From/$To pick the versions (0.7.0 -> 0.7.1: the phrase is made before the update and
# must still recover after it).
# Runs on the VM's real install and data folder; the earlier data folder is moved aside first and
# put back by the 'restore' phase. Test password and test phrase only: the phrase is copied into
# a file in the data folder (deleted by 'restore'), never logged, never photographed.
#   prep      move the data folder aside, uninstall, install the live 0.6.0 (hash checked), start
#   vault060  0.6.0: a vault with $PW060
#   logins    0.6.0: two logins (Add on the passwords page), then a card through the checkout
#   check060  0.6.0: the list, the vault format
#   update    restart 0.6.0, wait for the update prompt, Update now, wait for 0.7.0
#   after070  0.7.0: unlock with $PW060, the list (2 logins, 1 card), the vault format
#   phrase    0.7.0: create a recovery phrase, confirm the asked words ($Asked), format 3
#   recover   0.7.0: lock, recover with the phrase, new password $PWNEW; restart: $PW060 refused,
#             $PWNEW opens with the entries
#   restore   delete the test data folder, put the earlier one back
param([string]$Phase = 'prep', [string]$Asked = '', [string]$From = '0.6.0', [string]$To = '0.7.0')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "vg-$Phase"
$K = 'C:\pipeline\krunk'
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$data = Join-Path $env:LOCALAPPDATA 'Krunk'
$bak = Join-Path $env:LOCALAPPDATA 'Krunk.verify-bak'
$secret = "$data\phrase-under-test.txt"
$PW060 = 'Krunk-060-vault-test'
$PWNEW = 'Krunk-070-after-recovery'
function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { WindowRect $pw } else { $null }
}
function StartKrunk {
  Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $w = MainWindow
  [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $w | Out-Null
}
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
}
function Win { $w = MainWindow; Focus $w | Out-Null; $script:c = ClientRect $w }
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
function VaultInfo {
  python -c "import json,sys; d=json.load(open(sys.argv[1])); print('format', d.get('format'), '| recovery slot', bool(d.get('recovery')), '| keys', sorted(d))" "$data\vault.json"
}
switch ($Phase) {
  'prep' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
    if ((Test-Path $data) -and -not (Test-Path $bak)) { Move-Item $data $bak }
    elseif (Test-Path $data) { Remove-Item -Recurse -Force $data }
    Log "[$P] earlier data folder kept at $bak : $(Test-Path $bak); data folder now: $(Test-Path $data)"
    if (Test-Path "$inst\uninstall.exe") {
      Start-Process -FilePath "$inst\uninstall.exe" -ArgumentList '/S' -Wait
      for ($i = 0; $i -lt 60 -and (Test-Path "$inst\Krunk.exe"); $i++) { Start-Sleep 1 }
    }
    Log "[$P] uninstalled: $(-not (Test-Path "$inst\Krunk.exe")); registered version: '$(Version)'"
    $setup = "$K\Krunk-Setup-$From.exe"
    if (-not (Test-Path $setup)) { Invoke-WebRequest -UseBasicParsing "https://krunk.209.wf/download/$From/Krunk-Setup-$From.exe" -OutFile $setup }
    $sums = (Invoke-WebRequest -UseBasicParsing "https://krunk.209.wf/download/$From/SHA256SUMS").Content
    $want = ($sums -split "`n" | Where-Object { $_ -match "Krunk-Setup-$From.exe" }) -split ' ' | Select-Object -First 1
    Log "[$P] $From installer matches the site: $($want -eq (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLower())"
    $proc = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru; $proc.WaitForExit(300000) | Out-Null
    Log "[$P] installed $(Version) (exit $($proc.ExitCode)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
    StartKrunk; Win
    SafeGo 'krunk://passwords' 3000
    Shot "$P-1-passwords"
  }
  'vault060' {
    Win
    TypeSecret $PW060; Keys '{TAB}' 200; TypeSecret $PW060; Keys '{ENTER}' 4000
    Shot "$P-1-vault"
    Log "[$P] $(VaultInfo)"
  }
  'logins' {
    Win
    Click 832 145 1000                                  # Add
    TypeSecret 'https://bank.example'; Keys '{TAB}' 100; TypeSecret 'alice@example.com'; Keys '{TAB}' 100; TypeSecret 'Bank-pass-1'; Keys '{ENTER}' 1500
    Click 832 145 1000
    TypeSecret 'https://mail.example'; Keys '{TAB}' 100; TypeSecret 'bob'; Keys '{TAB}' 100; TypeSecret 'Mail-pass-2'; Keys '{ENTER}' 1500
    Shot "$P-1-two-logins"
    Click 336 314 1000                                  # Add card
    Shot "$P-2-card-form"
  }
  'card' {
    # 0.6.0 again (the vault is locked after its restart): unlock, Add card, each field clicked.
    StartKrunk; Win
    SafeGo 'krunk://passwords' 3000
    TypeSecret $PW060; Keys '{ENTER}' 3500
    Click 336 311 1200                                  # Add card
    Click 543 312 200; TypeSecret '4111 1111 1111 1111'
    Click 433 349 200; TypeSecret '12'
    Click 519 349 200; TypeSecret '2030'
    Click 577 387 200; TypeSecret 'Alice Example'
    Click 553 429 200; TypeSecret 'Test Visa'
    Shot "$P-0-form"
    Click 898 580 2000                                  # Save
    Shot "$P-1-listed"
    Log "[$P] $(VaultInfo)"
    Log "[$P] entries in the file: $(python -c "import json,sys; d=json.load(open(sys.argv[1])); print(len(d['entries']), 'sealed entries')" "$data\vault.json")"
    StopKrunk
    Log "[$P] 0.6.0 closed; vault.json $((Get-Item "$data\vault.json").Length) bytes, sha256 $((Get-FileHash "$data\vault.json").Hash.Substring(0,16))"
  }
  'update' {
    StartKrunk
    $t0 = Get-Date
    Log "[$P] running $(Version)"
    $r = $null
    for ($i = 0; $i -lt 150 -and -not $r; $i++) { Start-Sleep 1; $r = PopupRectNow }
    Log "[$P] prompt after $([int]((Get-Date) - $t0).TotalSeconds) s"
    Shot "$P-1-prompt"
    # The prompt's Update now (its right button); focusing the window first would close it.
    if ($r) { Click ($r.Right - 66) ($r.Top + 103) 3000 } else { Log "[$P] no prompt" }
    $t0 = Get-Date
    for ($i = 0; $i -lt 240 -and (Version) -ne $To; $i++) { Start-Sleep 1 }
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
    Start-Sleep 10
    Log "[$P] version $(Version) after $([int]((Get-Date) - $t0).TotalSeconds) s; Krunk.dll $((Get-FileHash "$inst\Krunk.dll").Hash.Substring(0,16)); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
    Log "[$P] vault.json untouched by the update: $((Get-FileHash "$data\vault.json").Hash.Substring(0,16))"
    Shot "$P-2-restarted"
  }
  'after070' {
    Win
    SafeGo 'krunk://passwords' 3000
    Shot "$P-1-locked"
    TypeSecret $PW060; Keys '{ENTER}' 4000
    Shot "$P-2-opened"
    Log "[$P] $(VaultInfo)"
    Log "[$P] version $(Version)"
    Log "[$P] log: $((Get-Content "$data\krunk.log" | Select-String 'vault|starting|release files|engine files' | Select-Object -Last 8) -join ' ## ')"
    Log "[$P] WARN/ERROR: $(@(Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ').Count) $((Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ' | Select-Object -Last 5) -join ' ## ')"
  }
  'phrase' {
    Win
    Click 1013 611 1000                                 # Create recovery phrase
    Shot "$P-1-master-password"
    TypeSecret $PW060; Keys '{ENTER}' 3000
    # Step 2 (the words; not photographed): Copy is 4 Shift+Tabs before "I have written them down".
    Set-Clipboard -Value 'empty'
    Keys '+{TAB}+{TAB}+{TAB}+{TAB}' 200; Keys '{ENTER}' 1200
    $words = (Get-Clipboard) -join ' '
    Set-Content $secret $words
    Set-Clipboard -Value 'cleared'
    Log "[$P] copied phrase: $(($words -split ' ').Count) words (kept in the test data folder only)"
    Keys '{TAB}{TAB}{TAB}{TAB}' 200; Keys '{ENTER}' 1000
    Shot "$P-2-asked"
  }
  'confirm' {
    Win
    $w = (Get-Content $secret) -split ' '
    $pos = $Asked -split ','
    TypeSecret $w[[int]$pos[0] - 1]; Keys '{TAB}' 100
    TypeSecret $w[[int]$pos[1] - 1]; Keys '{TAB}' 100
    TypeSecret $w[[int]$pos[2] - 1]; Keys '{ENTER}' 3000
    Shot "$P-1-confirmed"
    Log "[$P] $(VaultInfo)"
    $pw = (Get-Content $secret) -split ' '
    $triples = 0..21 | ForEach-Object { ($pw[$_..($_ + 2)]) -join ' ' }
    $hits = @(Select-String -Path "$data\vault.json", "$data\krunk.log", "$data\cef.log" -SimpleMatch -Pattern $triples -ErrorAction SilentlyContinue).Count
    Log "[$P] the phrase (any 3 consecutive words) in vault.json, krunk.log, cef.log: $hits matches"
  }
  'recover' {
    Win
    SafeGo 'krunk://passwords' 2500
    Click 1069 145 1500                                 # Lock
    Keys '{TAB}{TAB}' 200; Keys '{ENTER}' 1000          # Unlock form: password, Unlock, "Forgot master password?"
    Shot "$P-1-forgot"
    TypeSecret (Get-Content $secret)
    Start-Sleep 1
    Keys '{TAB}' 100; TypeSecret $PWNEW; Keys '{TAB}{TAB}' 100; TypeSecret $PWNEW
    # (No screenshot while the words are on screen: only the result.)
    Keys '{ENTER}' 5000
    Shot "$P-2-opened"
    Log "[$P] $(VaultInfo)"
    Log "[$P] log: $((Get-Content "$data\krunk.log" | Select-String 'vault|recovery' | Select-Object -Last 4) -join ' ## ')"
    # After a restart: the 0.6.0 password no longer opens it, the new one does.
    StopKrunk; StartKrunk; Win
    SafeGo 'krunk://passwords' 2500
    TypeSecret $PW060; Keys '{ENTER}' 3500
    Shot "$P-3-060-password-refused"
    Log "[$P] after the 0.6.0 password: $((Get-Content "$data\krunk.log" | Select-String 'vault' | Select-Object -Last 1))"
    Keys '^a' 100; TypeSecret $PWNEW; Keys '{ENTER}' 3500
    Shot "$P-4-new-password-opens"
    Log "[$P] after the new password: $((Get-Content "$data\krunk.log" | Select-String 'vault' | Select-Object -Last 1))"
    Log "[$P] $(VaultInfo)"
    Copy-Item "$data\krunk.log" "$Shots\$P-krunk.log"
  }
  'restore' {
    StopKrunk
    $pw = (Get-Content $secret) -split ' '
    $triples = 0..21 | ForEach-Object { ($pw[$_..($_ + 2)]) -join ' ' }
    $files = @(Get-ChildItem $data -Recurse -File | Where-Object { $_.Name -ne 'phrase-under-test.txt' }) + @(Get-ChildItem $Shots -File)
    $hits = @($files | Select-String -SimpleMatch -Pattern $triples -List -ErrorAction SilentlyContinue).Count
    Log "[$P] the phrase (any 3 consecutive words) in the data folder and the evidence shots/logs: $hits file(s)"
    Remove-Item -Recurse -Force $data
    if (Test-Path $bak) { Move-Item $bak $data }
    Log "[$P] test data folder deleted (with the test phrase file); earlier data folder back: $(Test-Path "$data\vault.json")"
  }
  default { Win; Shot "$P-1" }
}
Log "[$P] done"
