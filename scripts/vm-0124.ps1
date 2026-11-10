# Krunk 0.12.4 on the VM (installed build): a page's JavaScript dialogs are Krunk's own prompt,
# always above the window, and every way it ends answers the page. Phases (via a wrapper that
# sets $Phase; shots and run.log in C:\pipeline\krunk\shots):
#   install  Krunk-Setup-0.12.4.exe /S
#   dialogs  alert / confirm / prompt / Leave site? / a click elsewhere / a background tab /
#            an alert loop stopped with "Don't let this page show more dialogs"
param([string]$Phase = 'dialogs', [int]$X = 0, [int]$Y = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "0124-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$exe = "$inst\Krunk.exe"
$profile = 'C:\pipeline\krunk\p0124'
$www = 'C:\pipeline\krunk\www\8094'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class ZW {
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(PT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
}
'@

function EnsureServer {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-File', 'C:\pipeline\krunk\serve.ps1', '-PrefixList', 'http://*:8094/'
    Start-Sleep 2
  }
}
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 3
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 2
}
function StartKrunk {
  $old = $env:LOCALAPPDATA; $env:LOCALAPPDATA = $profile
  Start-Process -FilePath $exe -WorkingDirectory $inst
  $env:LOCALAPPDATA = $old
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $script:A | Out-Null
}
function Title { [TW]::Txt((MainWindow)) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function Popup { $pids = KrunkPids; [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1 }
# Is the dialog shown and really in front (the window at its centre is the prompt)?
function DialogState([string]$what) {
  $pw = Popup
  if (-not $pw) { Log "[$P] $($what): no prompt visible; title '$(Title)'"; return $false }
  $r = WindowRect $pw
  $pt = New-Object ZW+PT; $pt.X = [int](($r.Left + $r.Right) / 2); $pt.Y = [int](($r.Top + $r.Bottom) / 2)
  $hit = [ZW]::GetAncestor([ZW]::WindowFromPoint($pt), 2)
  $fg = [TW]::Cls([U]::GetForegroundWindow())
  Log "[$P] $($what): prompt at $($r.Left),$($r.Top) $($r.Right - $r.Left)x$($r.Bottom - $r.Top); on top: $($hit -eq $pw); foreground: $fg; title '$(Title)'"
  return $true
}
# The page's buttons (page origin at client y=104 with the bookmarks bar).
function PageClick([int]$x, [int]$y) { Click (8 + $x) (104 + $y) 1500 }

switch ($Phase) {
  'install' {
    StopKrunk
    $proc = Start-Process -FilePath C:\pipeline\krunk\Krunk-Setup-0.12.4.exe -ArgumentList '/S' -PassThru
    $proc.WaitForExit(180000) | Out-Null
    Log "[$P] setup exit $($proc.ExitCode); version $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk').DisplayVersion)"
  }
  'dialogs' {
    EnsureServer
    Copy-Item C:\pipeline\krunk\dialog.html "$www\dialog.html" -Force
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    StartKrunk
    SafeGo 'http://127.0.0.1:8094/dialog.html' 3000
    # 1 alert: shown in front; Enter = OK
    PageClick 130 42
    DialogState 'alert' | Out-Null
    Shot "$P-1-alert"
    Keys '{ENTER}' 1000
    Log "[$P] alert answered: '$(Title)'"
    # 2 confirm: Shift+Tab to Cancel, Enter
    PageClick 130 102
    DialogState 'confirm' | Out-Null
    Keys '+{TAB}' 200; Keys '{ENTER}' 1000
    Log "[$P] confirm answered: '$(Title)'"
    # 3 prompt: type, Enter
    PageClick 130 162
    DialogState 'prompt' | Out-Null
    Shot "$P-3-prompt"
    Keys 'Bob' 200; Keys '{ENTER}' 1000
    Log "[$P] prompt answered: '$(Title)'"
    # 4 a click elsewhere (into the page) while a confirm is open: the page gets its answer
    PageClick 130 102
    DialogState 'confirm-2' | Out-Null
    Click 900 600 1500
    Log "[$P] confirm after a click elsewhere: '$(Title)'; prompt still visible: $([bool](Popup))"
    # 5 Leave site?: type, then go elsewhere; Esc stays, Enter leaves
    PageClick 500 60; Keys 'unsaved' 300
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/pip.html#one' 2500
    DialogState 'leave' | Out-Null
    Shot "$P-5-leave"
    Keys '{ESC}' 1500
    Log "[$P] leave site, Esc: '$(Title)'"
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/pip.html#one' 2500
    DialogState 'leave-2' | Out-Null
    Keys '{ENTER}' 3000
    Log "[$P] leave site, Enter: '$(Title)'"
    # 6 a dialog from a background tab: answered unseen, the tab is not stuck
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/dialog.html' 3000
    PageClick 130 282
    SafeKeys '^t' 5000
    Log "[$P] background tab: prompt visible: $([bool](Popup))"
    SafeKeys '^w' 1500
    Log "[$P] background tab, back: '$(Title)'"
    # 7 an alert loop: the second offers the block; ticked, the loop ends unseen
    PageClick 130 222
    DialogState 'loop-1' | Out-Null
    Keys '{ENTER}' 1000
    DialogState 'loop-2' | Out-Null
    Shot "$P-7-loop-block"
    Keys "+{TAB}" 200; Keys " " 200; Keys "{TAB}" 200; Keys "{ENTER}" 1500
    Log "[$P] alert loop after the block: '$(Title)'; prompt visible: $([bool](Popup))"
    # 8 that page blocked: unsaved text, then leave: no dialog, and the page lets the user go
    PageClick 500 60; Keys 'unsaved' 300
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/pip.html#one' 3000
    Log "[$P] blocked page, leave: '$(Title)'; prompt visible: $([bool](Popup))"
    # 9 a fresh page: Leave site? shows again, without the block offer
    SafeGo 'http://127.0.0.1:8094/dialog.html' 3000
    PageClick 130 42; Keys '{ENTER}' 800
    PageClick 500 60; Keys 'unsaved' 300
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/pip.html#one' 2500
    DialogState 'leave-3' | Out-Null
    Shot "$P-9-leave-no-block"
    Keys '{ENTER}' 3000
    Log "[$P] leave site, Enter: '$(Title)'"
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  }
}
