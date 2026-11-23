# Krunk 0.7.0 release check: the in-app update from the live 0.6.0 to the live 0.7.0 on the VM's
# real data folder (a session and the 0.6.0 vault of the earlier update tests).
#   install  the live 0.6.0 installer (hash checked against the site's SHA256SUMS), /S, start
#            Krunk; the update prompt a minute later
#   accept   Enter on the prompt (Update now); the installer runs; Krunk comes back
#   after    version, libcef.dll / Krunk.dll hashes, the engine and release file checks in the log,
#            WARN/ERROR lines, the session, and the 0.6.0 vault unlocking with its password
param([string]$Phase = 'install')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "up070-$Phase"
$K = 'C:\pipeline\krunk'
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$data = Join-Path $env:LOCALAPPDATA 'Krunk'
function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { WindowRect $pw } else { $null }
}
switch ($Phase) {
  'install' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    $setup = "$K\Krunk-Setup-0.6.0.exe"
    if (-not (Test-Path $setup)) { Invoke-WebRequest -UseBasicParsing 'https://krunk.kruxsys.com/download/0.6.0/Krunk-Setup-0.6.0.exe' -OutFile $setup }
    $sums = (Invoke-WebRequest -UseBasicParsing 'https://krunk.kruxsys.com/download/0.6.0/SHA256SUMS').Content
    $want = ($sums -split "`n" | Where-Object { $_ -match 'Krunk-Setup-0.6.0.exe' }) -split ' ' | Select-Object -First 1
    Log "[$P] 0.6.0 installer matches the site: $($want -eq (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLower())"
    Log "[$P] vault bytes before: $((Get-Item "$data\vault.json").Length)"
    $proc = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru; $proc.WaitForExit(300000) | Out-Null
    Log "[$P] installed $(Version) (exit $($proc.ExitCode))"
    Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    $t0 = Get-Date
    Start-Sleep 5
    $w = MainWindow; [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null
    Shot "$P-1-started"
    $r = $null
    for ($i = 0; $i -lt 150 -and -not $r; $i++) { Start-Sleep 1; $r = PopupRectNow }
    Log "[$P] prompt after $([int]((Get-Date) - $t0).TotalSeconds) s: $(if ($r) { "$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)" } else { 'none' })"
    Shot "$P-2-prompt"
    Log "[$P] log: $((Get-Content "$data\krunk.log" | Select-String 'update' | Select-Object -Last 3) -join ' ## ')"
  }
  'accept' {
    # The prompt's Update now (its right button); focusing the window first would close it.
    $r = PopupRectNow
    if ($r) { Click ($r.Right - 66) ($r.Top + 103) 3000 } else { Log "[$P] no prompt" }
    Shot "$P-1-downloading"
    $t0 = Get-Date
    for ($i = 0; $i -lt 240 -and (Version) -ne '0.7.0'; $i++) { Start-Sleep 1 }
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
    Start-Sleep 8
    Log "[$P] version $(Version) after $([int]((Get-Date) - $t0).TotalSeconds) s; Krunk window: $((MainWindow) -ne [IntPtr]::Zero)"
    Shot "$P-2-restarted"
  }
  'after' {
    Start-Sleep 25
    Log "[$P] version: $(Version)"
    Log "[$P] libcef: $((Get-FileHash "$inst\libcef.dll").Hash)"
    Log "[$P] Krunk.dll: $((Get-FileHash "$inst\Krunk.dll").Hash)"
    Log "[$P] md files: $(@(Get-ChildItem $inst -Recurse -Filter *.md).Count); old-krunk files: $(@(Get-ChildItem $inst -Recurse -Filter *.old-krunk).Count); update-error.txt: $(Test-Path "$data\update-error.txt")"
    Log "[$P] checks: $((Get-Content "$data\krunk.log" | Select-String 'engine files|release files|session restored|Krunk 0.7.0 starting') -join ' ## ')"
    Log "[$P] WARN/ERROR: $(@(Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ').Count) $((Get-Content "$data\krunk.log" | Select-String ' WARN | ERROR ' | Select-Object -Last 5) -join ' ## ')"
    Focus (MainWindow) | Out-Null
    Shot "$P-1-restored"
    # The 0.6.0 vault of the earlier update tests (its password is not recorded): 0.7.0 must read
    # it — a wrong-password answer, not "damaged", and the file unchanged.
    SafeGo 'krunk://passwords' 3000
    Keys 'not-the-master-password' 200; Keys '{ENTER}' 4000
    Shot "$P-2-vault"
    Log "[$P] vault bytes after unlock: $((Get-Item "$data\vault.json").Length); log: $((Get-Content "$data\krunk.log" | Select-String 'vault' | Select-Object -Last 2) -join ' ## ')"
    SafeGo 'krunk://gpu' 3000
    Shot "$P-3-gpu"
  }
}
Log "[$P] done"
