# Krunk 0.12.3 on the VM (installed build): media of private tabs never reaches Windows' media
# controls. Phases (via a wrapper that sets $Phase; shots and run.log in C:\pipeline\krunk\shots):
#   install  Krunk-Setup-0.12.3.exe /S
#   media    a normal tab plays (session listed with its title), then a private tab plays
#            (nothing of it listed), then the private tab closes and the normal one plays again
param([string]$Phase = 'media', [int]$X = 0, [int]$Y = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "0123-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$exe = "$inst\Krunk.exe"
$profile = 'C:\pipeline\krunk\p0123'
$www = 'C:\pipeline\krunk\www\8094'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, $type) { $t = $asTask.MakeGenericMethod($type).Invoke($null, @($op)); $t.Wait(-1) | Out-Null; $t.Result }
[Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime] | Out-Null
[Windows.Storage.Streams.IRandomAccessStreamWithContentType, Windows.Storage.Streams, ContentType = WindowsRuntime] | Out-Null
[Windows.Storage.Streams.DataReader, Windows.Storage.Streams, ContentType = WindowsRuntime] | Out-Null
function Sessions([string]$what) {
  $mgr = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
  $n = 0
  foreach ($s in $mgr.GetSessions()) {
    $n++
    $props = Await ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
    Log "[$P] $($what): session $n app=$($s.SourceAppUserModelId) title='$($props.Title)' artist='$($props.Artist)' status=$($s.GetPlaybackInfo().PlaybackStatus)"
    if ($props.Thumbnail) {
      $st = Await ($props.Thumbnail.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType])
      $file = "$Shots\$P-$what-thumb-$n.png"
      [Windows.Storage.Streams.IInputStream, Windows.Storage.Streams, ContentType = WindowsRuntime] | Out-Null
      try {
        $in = [System.IO.WindowsRuntimeStreamExtensions]::AsStreamForRead([Windows.Storage.Streams.IInputStream]$st)
        $out = [IO.File]::Create($file); $in.CopyTo($out); $out.Close(); $in.Close()
      } catch { Log "[$P] $($what): session $n has a thumbnail (not readable here: $($_.Exception.Message))"; continue }
      Log "[$P] $($what): session $n thumbnail $($st.ContentType) $((Get-Item $file).Length) bytes sha256 $((Get-FileHash $file).Hash.Substring(0, 16))"
    } else { Log "[$P] $($what): session $n has no thumbnail" }
  }
  Log "[$P] $($what): $n session(s)"
}
# Volume up then down: Windows' flyout shows the media session (if there is one).
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KB { [DllImport("user32.dll")] public static extern void keybd_event(byte v, byte s, uint f, UIntPtr e); }'
function Flyout([string]$what) {
  [KB]::keybd_event(0xAF, 0, 0, [UIntPtr]::Zero); [KB]::keybd_event(0xAF, 0, 2, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 300
  [KB]::keybd_event(0xAE, 0, 0, [UIntPtr]::Zero); [KB]::keybd_event(0xAE, 0, 2, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 1200
  Shot "$P-$what-flyout"
  Start-Sleep 4
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
function Titles { (Frames | ForEach-Object { [TW]::Txt($_) }) -join ' | ' }

switch ($Phase) {
  'install' {
    StopKrunk
    $proc = Start-Process -FilePath C:\pipeline\krunk\Krunk-Setup-0.12.3.exe -ArgumentList '/S' -PassThru
    $proc.WaitForExit(180000) | Out-Null
    Log "[$P] setup exit $($proc.ExitCode); version $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk').DisplayVersion)"
  }
  'media' {
    EnsureServer
    Copy-Item C:\pipeline\krunk\smtc2.html "$www\smtc2.html" -Force
    Copy-Item C:\pipeline\krunk\long.mp4 "$www\long.mp4" -Force
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    StartKrunk
    SafeGo 'http://127.0.0.1:8094/smtc2.html' 3000
    Click 250 300 4000
    Log "[$P] $(Titles)"
    Sessions 'normal'
    Flyout 'normal'
    SafeKeys '^+n' 1500
    SafeGo 'http://127.0.0.1:8094/smtc2.html#private' 3000
    Click 250 300 4000
    Log "[$P] $(Titles)"
    Shot "$P-private-playing"
    Sessions 'private'
    Flyout 'private'
    SafeKeys '^w' 2000
    Focus (MainWindow) | Out-Null
    Click 250 300 1000
    Click 250 300 4000
    Log "[$P] $(Titles)"
    Sessions 'normal-again'
    Flyout 'normal-again'
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  }
}
