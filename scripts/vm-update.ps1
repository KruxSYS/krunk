# Update-flow checks on the VM (scripts/vm.sh ps1; needs lib.ps1, vm-install.ps1 and a dev
# build whose update server is reachable from the VM, e.g. through ssh -R). Writes run.log and
# screenshots with the prefix $Prefix.
#   -Mode install   silent install of $Setup, then start Krunk from the Start Menu shortcut
#   -Mode prompt    wait for the update prompt (first check runs 60 s after start)
#   -Mode accept    click "Update now" in the prompt and follow the update until Krunk is back
#   -Mode check     open krunk://settings, press "Check for updates", screenshot the About card
#   -Mode log       copy the update lines of krunk.log into run.log
param([string]$Mode = 'log', [string]$Prefix = 'm6up', [string]$Setup = 'C:\pipeline\krunk\Krunk-Setup-dev-0.2.0.exe')
. C:\pipeline\krunk\lib.ps1
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$log = Join-Path $env:LOCALAPPDATA 'Krunk\krunk.log'

function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function UpdateLines([int]$n = 12) {
  Get-Content $log -EA SilentlyContinue | Select-String 'update|starting|Krunk 0\.' | Select-Object -Last $n | ForEach-Object { Log "[$Prefix] log: $_" }
}
function StartKrunk {
  Start-Process (Join-Path ([Environment]::GetFolderPath('Programs')) 'Krunk.lnk')
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep -Seconds 3
}

switch ($Mode) {
  'install' {
    Get-Process Krunk -EA SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Seconds 4
    $p = Start-Process -FilePath $Setup -ArgumentList '/S' -PassThru
    $p.WaitForExit(240000) | Out-Null
    Log "[$Prefix] silent install of $Setup -> version $(Version)"
    StartKrunk
    Log "[$Prefix] Krunk started from the Start Menu shortcut: $((Get-Process Krunk -EA SilentlyContinue | Select-Object -First 1).Path)"
    Shot "$Prefix-01-started"
  }
  'prompt' {
    $h = [IntPtr]::Zero
    for ($i = 0; $i -lt 120; $i++) {
      $h = [U]::FindWindow('KrunkPopup', [NullString]::Value)
      $r = WindowRect $h
      if ($h -ne [IntPtr]::Zero -and ($r.Right - $r.Left) -gt 300 -and ($r.Bottom - $r.Top) -gt 100) {
        $vis = Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);' -Name V$i -PassThru
        if ($vis::IsWindowVisible($h)) { break }
      }
      Start-Sleep -Seconds 1
    }
    Start-Sleep -Milliseconds 800
    Log "[$Prefix] prompt after $i s: $($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
    Shot "$Prefix-02-prompt"
    UpdateLines 4
  }
  'accept' {
    $h = [U]::FindWindow('KrunkPopup', [NullString]::Value)
    $r = WindowRect $h
    $s = [U]::GetDpiForWindow($h) / 96.0
    # "Update now" is the right-most button of the bottom row.
    Click ([int]($r.Right - 60 * $s)) ([int]($r.Bottom - 47 * $s)) 1500
    Shot "$Prefix-03-downloading"
    $old = (Get-Process Krunk -EA SilentlyContinue | Select-Object -First 1).Id
    $t0 = Get-Date
    for ($i = 0; $i -lt 300; $i++) {
      Start-Sleep -Seconds 1
      if (-not (Get-Process -Id $old -EA SilentlyContinue)) { break }
      if ($i -eq 4) { Shot "$Prefix-04-progress" }
    }
    Log "[$Prefix] Krunk $old exited after $([int]((Get-Date) - $t0).TotalSeconds) s"
    for ($i = 0; $i -lt 180; $i++) {
      Start-Sleep -Seconds 1
      if ((Get-Process Krunk -EA SilentlyContinue) -and (MainWindow) -ne [IntPtr]::Zero) { break }
    }
    Start-Sleep -Seconds 4
    Log "[$Prefix] Krunk back after $([int]((Get-Date) - $t0).TotalSeconds) s: version $(Version), $((Get-Process Krunk -EA SilentlyContinue | Select-Object -First 1).Path)"
    Get-ChildItem "$env:TEMP\Krunk-update" -EA SilentlyContinue | ForEach-Object { Log "[$Prefix] left in TEMP: $($_.Name)" }
    Shot "$Prefix-05-restarted"
    UpdateLines 8
  }
  'check' {
    $h = MainWindow
    [U]::SetForegroundWindow($h) | Out-Null
    $c = ClientRect $h
    Click ([int](($c.Left + $c.Right) / 2)) ([int]($c.Top + 300 * $c.Scale)) 300
    Keys '^t' 1500
    Go 'krunk://settings' 2500
    Keys '^{END}' 800
    $c = ClientRect $h
    Shot "$Prefix-06-about"
    UpdateLines 4
  }
  'restart' {
    Get-Process Krunk -EA SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    for ($i = 0; $i -lt 30 -and (Get-Process Krunk -EA SilentlyContinue); $i++) { Start-Sleep -Seconds 1 }
    StartKrunk
    Start-Sleep -Seconds 70
    Shot "$Prefix-07-after-check"
    UpdateLines 6
  }
  default { UpdateLines 20 }
}
