# Krunk 0.12.1 on the VM (the installed build): default browser, launch addresses, the media
# overlay's artwork and the Picture-in-Picture chooser. Run phase by phase through
# scripts/vm.sh ps1 (the wrapper sets $Phase; screenshots and run.log under C:\pipeline\krunk\shots):
#   install   Krunk-Setup-0.12.1.exe /S over the installed version; the registration in HKCU
#   settings  start the installed Krunk on a fresh profile, krunk://settings (the banner)
#   make      click the banner's Make default (Windows Settings opens at Krunk's page)
#   setdef    click Windows Settings' "Set default" at ($X,$Y), then back to krunk://settings
#   links     open a link and a file from Windows while Krunk runs, then with Krunk closed
#   media     a video with sound and no artwork, then the volume key's media flyout
#   smtc      the system media sessions (WinRT): app, title, and the thumbnail saved as a file
#   pip       two videos: the toolbar's PiP button opens the chooser
#   restore   Krunk closed, Edge made the default again at ($X,$Y) (the VM is shared)
param([string]$Phase = 'settings', [int]$X = 0, [int]$Y = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "0121-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$exe = "$inst\Krunk.exe"
$profile = 'C:\pipeline\krunk\p0121'
$www = 'C:\pipeline\krunk\www\8094'

function Reg([string]$k, [string]$n) { (Get-ItemProperty -Path "HKCU:\$k" -Name $n -ErrorAction SilentlyContinue).$n }
function Defaults {
  foreach ($s in 'http', 'https') {
    Log "[$P] UserChoice $s = $(Reg "Software\Microsoft\Windows\Shell\Associations\UrlAssociations\$s\UserChoice" 'ProgId')"
  }
  Log "[$P] UserChoice .html = $(Reg 'Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.html\UserChoice' 'ProgId')"
}
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
function Arrange {
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $script:A | Out-Null
}
# The installed Krunk on the test profile (LOCALAPPDATA only for Krunk: it is installed under
# the real one).
function StartKrunk([string[]]$KArgs = @()) {
  $old = $env:LOCALAPPDATA
  $env:LOCALAPPDATA = $profile
  if ($KArgs.Count) { Start-Process -FilePath $exe -ArgumentList $KArgs -WorkingDirectory $inst } else { Start-Process -FilePath $exe -WorkingDirectory $inst }
  $env:LOCALAPPDATA = $old
  Arrange
}
function Titles { (Frames | ForEach-Object { [TW]::Txt($_) }) -join ' | ' }

switch ($Phase) {
  'install' {
    StopKrunk
    $before = Reg 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' 'DisplayVersion'
    $proc = Start-Process -FilePath C:\pipeline\krunk\Krunk-Setup-0.12.1.exe -ArgumentList '/S' -PassThru
    $proc.WaitForExit(180000) | Out-Null
    Log "[$P] setup exit $($proc.ExitCode); version $before -> $(Reg 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' 'DisplayVersion')"
    Log "[$P] open command: $(Reg 'Software\Classes\KrunkHTML\shell\open\command' '(default)')"
    Log "[$P] RegisteredApplications Krunk: $(Reg 'Software\RegisteredApplications' 'Krunk')"
    Log "[$P] URLAssociations https: $(Reg 'Software\Clients\StartMenuInternet\Krunk\Capabilities\URLAssociations' 'https')"
    Log "[$P] FileAssociations .html: $(Reg 'Software\Clients\StartMenuInternet\Krunk\Capabilities\FileAssociations' '.html')"
    Log "[$P] .htm OpenWithProgids: $((Get-Item 'HKCU:\Software\Classes\.htm\OpenWithProgids' -ErrorAction SilentlyContinue).Property -join ',')"
    Log "[$P] resources.pak $((Get-FileHash "$inst\resources.pak").Hash.Substring(0, 16))"
    Defaults
  }
  'settings' {
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    StartKrunk
    SafeGo 'krunk://settings' 3000
    Shot "$P-banner"
    Defaults
  }
  'make' {
    Focus (MainWindow) | Out-Null
    # The banner's Make default (right end of the banner).
    Click $X $Y 4000
    Shot "$P-windows-settings"
    $sys = Get-Process SystemSettings -ErrorAction SilentlyContinue
    Log "[$P] SystemSettings running: $([bool]$sys)"
  }
  'setdef' {
    Click $X $Y 3000
    Shot "$P-after-set-default"
    Defaults
    Get-Process SystemSettings -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep 1
    Focus (MainWindow) | Out-Null
    Start-Sleep 3
    Shot "$P-krunk-settings"
  }
  'links' {
    # One profile for every Krunk.exe here: the one Windows starts uses the real LOCALAPPDATA
    # (a second profile would be a second, separate Krunk).
    $profile = $env:LOCALAPPDATA
    StopKrunk
    EnsureServer
    Copy-Item 'C:\pipeline\krunk\launch page.html' "$env:TEMP\launch page.html" -Force
    # Krunk running: Windows hands the link to it (a second Krunk.exe forwards it).
    if ((MainWindow) -eq [IntPtr]::Zero) { StartKrunk }
    Start-Process 'http://127.0.0.1:8094/pip.html#one'
    Start-Sleep 4
    # Windows asks once which app opens http links after new ones were registered (Krunk is
    # already marked as the default there): "Always".
    if (Get-Process OpenWith -ErrorAction SilentlyContinue) {
      Shot "$P-openwith"
      Click 539 619 4000
      Log "[$P] Windows' app chooser answered (Always)"
    }
    Log "[$P] running, link: $(Titles)"
    Shot "$P-running-link"
    & $exe --single-argument "$env:TEMP\launch page.html"
    Start-Sleep 5
    Log "[$P] running, file: $(Titles)"
    Shot "$P-running-file"
    # Krunk closed: the link starts it (first start with the address).
    StopKrunk
    $old = $env:LOCALAPPDATA; $env:LOCALAPPDATA = $profile
    Start-Process -FilePath $exe -ArgumentList '--single-argument', "$env:TEMP\launch page.html" -WorkingDirectory $inst
    $env:LOCALAPPDATA = $old
    Arrange
    Log "[$P] closed, file: $(Titles)"
    Shot "$P-closed-file"
    StopKrunk
    $old = $env:LOCALAPPDATA; $env:LOCALAPPDATA = $profile
    Start-Process 'http://127.0.0.1:8094/pip.html#one'
    $env:LOCALAPPDATA = $old
    Arrange
    Log "[$P] closed, link: $(Titles)"
    Shot "$P-closed-link"
    # Refused: a javascript: address opens nothing new.
    & $exe --single-argument 'javascript:alert(1)'
    Start-Sleep 4
    Log "[$P] javascript: $(Titles)"
    Shot "$P-javascript"
  }
  'media' {
    EnsureServer
    Copy-Item C:\pipeline\krunk\smtc.html "$www\smtc.html" -Force
    if ((MainWindow) -eq [IntPtr]::Zero) { StartKrunk }
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/smtc.html' 3000
    Click 640 400 3000
    Log "[$P] title: $(Titles)"
    # Volume up then down: Windows shows its media flyout with the playing session.
    Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KB { [DllImport("user32.dll")] public static extern void keybd_event(byte v, byte s, uint f, UIntPtr e); }'
    [KB]::keybd_event(0xAF, 0, 0, [UIntPtr]::Zero); [KB]::keybd_event(0xAF, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    [KB]::keybd_event(0xAE, 0, 0, [UIntPtr]::Zero); [KB]::keybd_event(0xAE, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 900
    Shot "$P-flyout"
  }
  'smtc' {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
    function Await($op, $type) { $t = $asTask.MakeGenericMethod($type).Invoke($null, @($op)); $t.Wait(-1) | Out-Null; $t.Result }
    [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime] | Out-Null
    [Windows.Storage.Streams.IRandomAccessStreamWithContentType, Windows.Storage.Streams, ContentType = WindowsRuntime] | Out-Null
    $mgr = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
    $n = 0
    foreach ($s in $mgr.GetSessions()) {
      $n++
      $props = Await ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
      Log "[$P] session $n app=$($s.SourceAppUserModelId) title='$($props.Title)' artist='$($props.Artist)' status=$($s.GetPlaybackInfo().PlaybackStatus)"
      if ($props.Thumbnail) {
        $st = Await ($props.Thumbnail.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType])
        $in = [System.IO.WindowsRuntimeStreamExtensions]::AsStreamForRead($st)
        $file = "$Shots
1-smtc-thumb-$n.png"
        $out = [IO.File]::Create($file); $in.CopyTo($out); $out.Close(); $in.Close()
        Log "[$P] session $n thumbnail $($st.ContentType) $((Get-Item $file).Length) bytes sha256 $((Get-FileHash $file).Hash.Substring(0, 16))"
      } else { Log "[$P] session $n has no thumbnail" }
    }
    Log "[$P] $n session(s)"
  }
  'restore' {
    StopKrunk
    Start-Process 'ms-settings:defaultapps?registeredAppMachine=Microsoft Edge'
    Start-Sleep 4
    # Settings opens cascaded: put it where its "Set default" is at (694, 147).
    $h = [U]::FindWindow('ApplicationFrameWindow', 'Settings')
    [U]::MoveWindow($h, 0, 0, 800, 632, $true) | Out-Null; Start-Sleep 1
    Shot "$P-edge-page"
    Click 694 147 3000
    Shot "$P-edge-set"
    Get-Process SystemSettings -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item -Recurse -Force $profile, "$env:TEMP\launch page.html" -ErrorAction SilentlyContinue
  }
  'pip' {
    EnsureServer
    StopKrunk
    StartKrunk
    Focus (MainWindow) | Out-Null
    SafeGo 'http://127.0.0.1:8094/pip.html?two' 3500
    Shot "$P-page"
    Click $X $Y 1500
    Shot "$P-chooser"
    # The chooser has the focus (a KrunkPopup): plain keys.
    Keys '{ESC}' 500
  }
}
