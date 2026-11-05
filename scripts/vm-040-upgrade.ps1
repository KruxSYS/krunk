# Krunk 0.4.0 release check: upgrade from the live 0.3.0 with a multi-tab session, two windows
# and a finished download (0.4.0 reads 0.3.0's download list and session).
#   -Phase prep         back up browser.db/vault.json, install the live 0.3.0 (/S, hash checked),
#                       open tabs in two windows, bookmark one, download a file, quit
#   -Phase silent       install C:\pipeline\krunk\Krunk-Setup-0.4.0.exe with /S, list the folder,
#                       start Krunk: both windows come back; the downloads page lists the file
#   -Phase reprep       put 0.3.0 back (/S) and run it once
#   -Phase interactive  the 0.4.0 wizard (Next, Next, Install, Finish), Finish starts Krunk
#   -Phase restore      put 0.3.0 back and the data backed up in prep (leaves the VM as found)
# Needs serve.ps1 on localhost:8092. Logs to run.log, screenshots "up-*". Copies browser.db
# after each phase to C:\pipeline\krunk\up-<phase>.db for the data comparison.
param([string]$Phase = 'prep')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "up-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$data = Join-Path $env:LOCALAPPDATA 'Krunk'
$old = 'C:\pipeline\krunk\Krunk-Setup-0.3.0-live.exe'
$new = 'C:\pipeline\krunk\Krunk-Setup-0.4.0.exe'

function Listing {
  $md = @(Get-ChildItem $inst -Recurse -File -Filter *.md -ErrorAction SilentlyContinue)
  Log "[$P] install folder: $(@(Get-ChildItem $inst -File).Count) files at the top; *.md anywhere: $($md.Count) $(($md | ForEach-Object Name) -join ', ')"
  Log "[$P] README.md: $(Test-Path "$inst\README.md"), SECURITY.md: $(Test-Path "$inst\SECURITY.md")"
  Log "[$P] uninstall entry version: $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion)"
  Log "[$P] vault.json SHA-256: $((Get-FileHash "$data\vault.json" -EA SilentlyContinue).Hash)"
}
function Install([string]$setup) {
  Get-Process Krunk -EA SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 3
  $proc = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru
  $proc.WaitForExit(180000) | Out-Null
  Log "[$P] $setup /S exit code $($proc.ExitCode)"
  Start-Sleep 2
}
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class D {
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@
# Presses a wizard button without needing the keyboard focus.
function Press([IntPtr]$h, [int]$id = 1, [int]$wait = 1200) {
  $b = [D]::GetDlgItem($h, $id); [D]::SendMessage($b, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds $wait
}
function Start-Installed {
  Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 8
}
function Quit {
  foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 2000 } }
  for ($i = 0; $i -lt 20 -and (Get-Process Krunk -EA SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Log "[$P] Krunk processes after quitting: $(@(Get-Process Krunk -EA SilentlyContinue).Count)"
  Copy-Item "$data\browser.db" "C:\pipeline\krunk\$P.db" -Force
}
function Downloads {
  $A = MainWindow; Focus $A | Out-Null
  SafeKeys '^j' 2500
  Shot "$P-downloads"
}
# Exit from the main menu of window $h: every window closes at once and all are in the session.
function ExitAll([IntPtr]$h) {
  if (-not (Focus $h)) { throw 'no focus' }
  $ca = ClientRect $h
  Click ([int]($ca.Right - 22)) ([int]($ca.Top + 56)) 1200
  Shot "$P-menu"
  # Exit is the last item: popup top = toolbar bottom (76 DIP) + 6 + 4 groups of items and separators.
  Click ([int]($ca.Right - 160)) ([int]($ca.Top + (76 + 504) * $ca.Scale)) 3000
  for ($i = 0; $i -lt 20 -and (Get-Process Krunk -EA SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Log "[$P] Krunk processes after Exit: $(@(Get-Process Krunk -EA SilentlyContinue).Count)"
  Copy-Item "$data\browser.db" "C:\pipeline\krunk\$P.db" -Force
}
function Restored {
  KrunkWindows $P
  Shot "$P-restored"
  Get-Content "$data\krunk.log" | Select-String 'starting|session|version|WARN|ERROR' | ForEach-Object { Log "[$P] log: $_" }
}
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}

switch ($Phase) {
  'prep' {
    $bak = 'C:\pipeline\krunk\data-backup-040'
    New-Item -ItemType Directory -Force $bak | Out-Null
    Copy-Item "$data\browser.db", "$data\vault.json" $bak -Force
    Log "[$P] backed up browser.db and vault.json to $bak"
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest 'https://krunk.209.wf/download/0.3.0/Krunk-Setup-0.3.0.exe' -OutFile $old -UseBasicParsing
    $sums = (Invoke-WebRequest 'https://krunk.209.wf/download/0.3.0/SHA256SUMS' -UseBasicParsing).Content
    $want = ($sums -split "`n" | Where-Object { $_ -match 'Krunk-Setup-0.3.0.exe' }) -replace '\s.*$', ''
    $have = (Get-FileHash $old).Hash.ToLower()
    Log "[$P] live 0.3.0 installer SHA-256 $have, SHA256SUMS says $want, match: $($have -eq $want)"
    if ($have -ne $want) { throw 'hash mismatch' }
    Install $old
    Listing
    Start-Installed
    $A = MainWindow
    [U]::MoveWindow($A, 40, 30, 1200, 720, $true) | Out-Null; Start-Sleep 1
    if (-not (Focus $A)) { throw 'no focus' }
    SafeGo 'http://localhost:8092/move.html?upgrade=1' 3000
    SafeKeys '^t' 1200; SafeGo 'https://example.com/' 3000
    SafeKeys '^d' 1500
    SafeKeys '^t' 1200; SafeGo 'http://localhost:8092/files/small.bin' 4000
    SafeKeys '^t' 1200; SafeGo 'krunk://history' 2500
    SafeKeys '^n' 2500; SafeGo 'http://localhost:8092/move.html?w=second' 3000
    Shot "$P-030-tabs"
    KrunkWindows $P
    ExitAll ([U]::GetForegroundWindow())
  }
  'silent' {
    Install $new
    Listing
    Start-Installed
    Restored
    Downloads
    Quit
  }
  'reprep' {
    Install $old
    Listing
    Start-Installed
    Shot "$P-030-again"
    KrunkWindows $P
    ExitAll (MainWindow)
  }
  'restore' {
    Install $old
    Copy-Item 'C:\pipeline\krunk\data-backup-040\browser.db', 'C:\pipeline\krunk\data-backup-040\vault.json' $data -Force
    Listing
    Log "[$P] restored 0.3.0 and the data from prep"
  }
  'interactive' {
    if (Get-Process Krunk-Setup* -EA SilentlyContinue) { Log "[$P] a setup is already running"; throw 'setup running' }
    # The wizard, driven with BM_CLICK (1 = Next/Install/Finish). An upgrade already has
    # uninstall.exe, so the install is done when the uninstall entry says 0.4.0.
    $proc = Start-Process -FilePath $new -PassThru
    # This installer's own wizard (not one left over from an earlier run).
    $h = [IntPtr]::Zero
    for ($i = 0; $i -lt 80 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 250; $proc.Refresh(); $h = $proc.MainWindowHandle }
    if ($h -eq [IntPtr]::Zero) { Log "[$P] no wizard"; throw 'no wizard' }
    Start-Sleep 1; Shot "$P-1-welcome"
    Press $h; Shot "$P-2-components"
    Press $h; Shot "$P-3-directory"
    Press $h 1 500
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk'
    for ($i = 0; $i -lt 240 -and (Get-ItemProperty $key -EA SilentlyContinue).DisplayVersion -ne '0.4.0'; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 4; Shot "$P-4-installed"
    Press $h; Shot "$P-5-finish"
    Press $h 1 1000
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 8
    Log "[$P] installer exit code: $(if ($proc.HasExited) { $proc.ExitCode } else { 'running' }); setup processes left: $(@(Get-Process Krunk-Setup* -EA SilentlyContinue).Count)"
    Listing
    Restored
    Downloads
    Quit
  }
}
