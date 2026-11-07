# Krunk 0.7.0 M7 (R13, R14): the recovery phrase on the real vault (DPAPI) of a throwaway test
# profile. Phases (Krunk stays open between them; -Phase from vm-batch's sed):
#   setup    new profile, vault with master password A, Settings link name, the change-password
#            dialog (light) and the empty recovery card
#   make     Create recovery phrase: master password, the words (copied to a file in the test
#            profile through the page's Copy button: never into the evidence), step 3 shown
#   confirm  the asked positions ($Asked) typed back from that file
#   recover  lock; "Forgot master password?"; the phrase from the file; new master password B;
#            the vault opens with the entries; B unlocks after a restart, A does not
#   renew    a new phrase; the old one is refused
#   dark     the dialogs in the dark theme
param([string]$Phase = 'setup', [string]$Asked = '')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "rec-$Phase"
$profile = "$K\p070rec"
$secret = "$profile\phrase-under-test.txt"
$env:LOCALAPPDATA = $profile
$A_PW = 'correct horse battery A'
$B_PW = 'second master password B'
function StartKrunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $w = MainWindow
  [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $w | Out-Null
}
function Win { $w = MainWindow; Focus $w | Out-Null; $script:c = ClientRect $w }
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
switch ($Phase) {
  'setup' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    StartKrunk; Win
    SafeGo 'krunk://passwords' 2500
    TypeSecret $A_PW; Keys '{TAB}' 200; TypeSecret $A_PW; Keys '{ENTER}' 4000
    Shot "$P-1-vault"
  }
  'make' {
    Win
    $renew = Test-Path $secret
    if ($renew) {
      Move-Item -Force $secret "$profile\phrase-under-test-old.txt"
      Click 956 606 1000                                # Make a new phrase
      $pwNow = $B_PW
    } else {
      Click 825 145 800                                 # Add: one login, so the recovery shows it
      TypeSecret 'https://bank.example'; Keys '{TAB}' 100; TypeSecret 'alice'; Keys '{TAB}' 100; TypeSecret 'Tr0ub4dor-3'; Keys '{ENTER}' 1500
      Click 1013 615 1000                               # Create recovery phrase (the list has one login now)
      $pwNow = $A_PW
    }
    Shot "$P-1-master-password"
    TypeSecret $pwNow; Keys '{ENTER}' 3000
    # Step 2 (the words; not photographed): Copy is 4 Shift+Tabs before "I have written them down".
    Set-Clipboard -Value 'empty'
    Keys '+{TAB}+{TAB}+{TAB}+{TAB}' 200; Keys '{ENTER}' 1200
    $words = (Get-Clipboard) -join ' '
    Set-Content $secret $words
    Log "[$P] copied phrase: $(($words -split ' ').Count) words (kept in the test profile only)"
    Keys '{TAB}{TAB}{TAB}{TAB}' 200; Keys '{ENTER}' 1000
    Shot "$P-2-asked"
  }
  'confirm' {
    Win
    $w = (Get-Content $secret) -split ' '
    $pos = $Asked -split ','
    TypeSecret $w[[int]$pos[0] - 1]; Keys '{TAB}' 100
    TypeSecret 'wrongword'; Keys '{TAB}' 100
    TypeSecret $w[[int]$pos[2] - 1]; Keys '{ENTER}' 1500
    Keys '+{TAB}' 100; Keys '^a' 50; TypeSecret $w[[int]$pos[1] - 1]; Keys '{ENTER}' 3000
    Shot "$P-2-confirmed"
    Log "[$P] stored: $(python -c "import json,sys; d=json.load(open(sys.argv[1])); r=d.get('recovery'); print('format', d['format'], 'recovery slot', bool(r), 'params', r and r['kdf'])" "$profile\Krunk\vault.json")"
    # Any three consecutive words of the phrase (single words are common English: "engine").
    $pw = (Get-Content $secret) -split ' '
    $triples = 0..21 | ForEach-Object { ($pw[$_..($_ + 2)]) -join ' ' }
    $hits = @(Select-String -Path "$profile\Krunk\vault.json", "$profile\Krunk\krunk.log", "$profile\Krunk\cef.log" -SimpleMatch -Pattern $triples -ErrorAction SilentlyContinue).Count
    Log "[$P] the phrase (any 3 consecutive words) in vault.json, krunk.log, cef.log: $hits matches"
  }
  'recover' {
    Win
    SafeGo 'krunk://passwords' 2500
    Click 1069 145 1500                                 # Lock (if open)
    Keys '{TAB}{TAB}' 200; Keys '{ENTER}' 1000          # Unlock form: password, Unlock, "Forgot master password?"
    Shot "$P-1-forgot"
    TypeSecret (Get-Content $secret)
    Start-Sleep 1
    Keys '{TAB}' 100; TypeSecret $B_PW; Keys '{TAB}{TAB}' 100; TypeSecret $B_PW
    # (No screenshot while the words are on screen: only the result.)
    Keys '{ENTER}' 5000
    Shot "$P-3-opened"
    Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'vault|recovery') -join ' ## ')"
    # After a restart: A no longer opens it, B does.
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
    StartKrunk; Win
    SafeGo 'krunk://passwords' 2500
    TypeSecret $A_PW; Keys '{ENTER}' 3000
    Shot "$P-4-old-password-refused"
    TypeSecret $B_PW; Keys '{ENTER}' 3000
    Shot "$P-5-new-password-opens"
  }
  'oldphrase' {
    # After a new phrase was made: the old one (phrase-under-test-old.txt) is refused.
    Win
    SafeGo 'krunk://passwords' 2500
    Click 1069 145 1500
    Keys '{TAB}{TAB}' 200; Keys '{ENTER}' 1000
    TypeSecret (Get-Content "$profile\phrase-under-test-old.txt")
    Start-Sleep 1
    Keys '{TAB}' 100; TypeSecret 'third password C'; Keys '{TAB}{TAB}' 100; TypeSecret 'third password C'; Keys '{ENTER}' 5000
    # (Not photographed: the old words are on screen.) The log says why it failed.
    Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'vault|recovery' | Select-Object -Last 4) -join ' ## ')"
  }
  'dialogs' {
    # The change-password dialog (R14) in the light theme, then dark after a restart.
    foreach ($theme in 'light', 'dark') {
      Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
      Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
      Set-Content "$K\rec-patch.json" "{`"theme`": `"$theme`"}"
      Log "[$P] theme: $(python "$K\setkv.py" $profile "@$K\rec-patch.json" 2>&1)"
      StartKrunk; Win
      SafeGo 'krunk://passwords' 2500
      TypeSecret $B_PW; Keys '{ENTER}' 3000
      Click 950 145 800                                 # Change master password
      TypeSecret 'not the password'; Keys '{TAB}{TAB}' 100; TypeSecret 'short'; Keys '{ENTER}' 600
      Shot "$P-change-$theme-errors"
      Keys '{ESC}' 400
      Click 956 606 800                                 # Make a new phrase: step 1 only
      Shot "$P-recovery-$theme-step1"
      Keys '{ESC}' 400
    }
  }
  default { Win }
}
Log "[$P] done"
