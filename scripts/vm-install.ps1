# Installer checks on the VM (run through scripts/vm.sh ps1 after copying lib.ps1 and the
# installer to C:\pipeline\krunk\). Drives the wizard with the keyboard and writes run.log plus
# screenshots with the prefix $Prefix.
#   -Mode install     interactive install (desktop shortcut ticked), Finish starts Krunk
#   -Mode uninstall   interactive uninstall, leaving "delete my data" unticked
#   -Mode state       log install folder, shortcuts, uninstall entry, data folder, processes
param([string]$Mode = 'state', [string]$Prefix = 'm6', [string]$Setup = 'C:\pipeline\krunk\Krunk-Setup.exe')
. C:\pipeline\krunk\lib.ps1
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk'

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class D {
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
}
'@
# Presses a wizard button (1 = Next/Install/Finish) without needing the keyboard focus.
function Press([IntPtr]$h, [int]$id = 1, [int]$wait = 1200) {
  $b = [D]::GetDlgItem($h, $id); [D]::SendMessage($b, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds $wait
}
# The components tree is inside the page's child dialog.
function Tree([IntPtr]$h) {
  $page = [D]::FindWindowEx($h, [IntPtr]::Zero, '#32770', [NullString]::Value)
  [D]::FindWindowEx($page, [IntPtr]::Zero, 'SysTreeView32', [NullString]::Value)
}

function Wizard([string]$title) {
  # The uninstaller first copies itself to %TEMP% and starts again from there.
  for ($i = 0; $i -lt 160; $i++) {
    $h = [U]::FindWindow('#32770', $title)
    # (Older uninstallers have a trailing space in their title.)
    if ($h -eq [IntPtr]::Zero) { $h = [U]::FindWindow('#32770', "$title ") }
    if ($h -ne [IntPtr]::Zero) { [U]::SetForegroundWindow($h) | Out-Null; Start-Sleep -Milliseconds 400; return $h }
    Start-Sleep -Milliseconds 250
  }
  Log "[$Prefix] window '$title' not found"; return [IntPtr]::Zero
}

function State {
  Log "[$Prefix] install folder: $(Test-Path "$inst\Krunk.exe") ($inst), uninstall.exe: $(Test-Path "$inst\uninstall.exe")"
  $sm = Join-Path ([Environment]::GetFolderPath('Programs')) 'Krunk.lnk'
  $dt = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Krunk.lnk'
  Log "[$Prefix] start menu shortcut: $(Test-Path $sm), desktop shortcut: $(Test-Path $dt)"
  if (Test-Path $key) {
    $k = Get-ItemProperty $key
    Log "[$Prefix] uninstall entry: $($k.DisplayName) $($k.DisplayVersion) by $($k.Publisher), size $($k.EstimatedSize) KB, '$($k.UninstallString)' / '$($k.QuietUninstallString)'"
  } else { Log "[$Prefix] uninstall entry: none" }
  Log "[$Prefix] HKLM entry: $(Test-Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk')"
  Log "[$Prefix] data folder: $(Test-Path "$env:LOCALAPPDATA\Krunk\browser.db")"
  $p = Get-Process Krunk -EA SilentlyContinue | Select-Object -First 1
  if ($p) { Log "[$Prefix] Krunk running from $($p.Path)" } else { Log "[$Prefix] Krunk not running" }
  $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  Log "[$Prefix] this session elevated: $admin"
}

switch ($Mode) {
  'install' {
    $p = Start-Process -FilePath $Setup -PassThru
    $h = Wizard 'Krunk Setup'; Shot "$Prefix-01-welcome"
    Press $h; Shot "$Prefix-02-components"
    # Tick "Desktop shortcut" (second item): click its label, then Space.
    $t = WindowRect (Tree $h)
    Click ($t.Left + 70) ($t.Top + 28) 400; Keys ' ' 400; Shot "$Prefix-03-components-desktop"
    Press $h; Shot "$Prefix-04-directory"
    Press $h 1 500
    for ($i = 0; $i -lt 240; $i++) { Start-Sleep -Milliseconds 500; if (Test-Path "$inst\uninstall.exe") { break } }
    Start-Sleep -Seconds 4; Shot "$Prefix-05-installed"
    Press $h; Shot "$Prefix-06-finish"
    Press $h 1 8000
    Log "[$Prefix] installer exit code: $(if ($p.HasExited) { $p.ExitCode } else { 'running' })"
    Shot "$Prefix-07-krunk-started"
    State
  }
  'uninstall' {
    Start-Process -FilePath "$inst\uninstall.exe"
    $h = Wizard 'Krunk Uninstall'; Shot "$Prefix-11-uninstall-confirm"
    Press $h; Shot "$Prefix-12-uninstall-components"
    Press $h 1 500
    for ($i = 0; $i -lt 120; $i++) { Start-Sleep -Milliseconds 500; if (-not (Test-Path "$inst\Krunk.exe")) { break } }
    Start-Sleep -Seconds 3; Shot "$Prefix-13-uninstalled"
    Press $h
    State
  }
  default { State }
}
