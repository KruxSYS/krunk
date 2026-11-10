# Krunk 0.12.2 on the VM (installed build): the page context menu's image items.
# Phases (scripts/vm.sh ps1 via a wrapper that sets $Phase; shots and run.log in C:\pipeline\krunk\shots):
#   install  Krunk-Setup-0.12.2.exe /S
#   menu     image.html; right-click the served image: the menu
#   items    each item in turn on the served image ($X,$Y: the first item), then Copy image on
#            the data: image; the clipboard and the saved file are checked
param([string]$Phase = 'menu', [int]$X = 0, [int]$Y = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "0122-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$exe = "$inst\Krunk.exe"
$profile = 'C:\pipeline\krunk\p0122'
$www = 'C:\pipeline\krunk\www\8094'
Add-Type -AssemblyName System.Windows.Forms

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
function ClipState([string]$what) {
  $img = [System.Windows.Forms.Clipboard]::GetImage()
  $fmts = ([System.Windows.Forms.Clipboard]::GetDataObject().GetFormats()) -join ','
  $txt = [System.Windows.Forms.Clipboard]::GetText()
  if ($txt.Length -gt 80) { $txt = $txt.Substring(0, 80) + '…' }
  $size = if ($img) { "$($img.Width)x$($img.Height)" } else { 'none' }
  Log "[$P] $what clipboard: image $size; formats $fmts; text '$txt'"
  if ($img) { $img.Save("$Shots\$P-$what-clip.png") }
}
# The served image's centre (page at y=104 in a 1280x760 window: bookmarks bar shown).
$SX = 168; $SY = 104 + 168
$DX = 464; $DY = 104 + 104

switch ($Phase) {
  'install' {
    StopKrunk
    $proc = Start-Process -FilePath C:\pipeline\krunk\Krunk-Setup-0.12.2.exe -ArgumentList '/S' -PassThru
    $proc.WaitForExit(180000) | Out-Null
    Log "[$P] setup exit $($proc.ExitCode); version $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk').DisplayVersion)"
  }
  'menu' {
    EnsureServer
    Copy-Item C:\pipeline\krunk\image.html "$www\image.html" -Force
    Copy-Item C:\pipeline\krunk\krunk-256.png "$www\krunk-256.png" -Force
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    StartKrunk
    SafeGo 'http://127.0.0.1:8094/image.html' 3000
    RightClick $SX $SY 1200
    Shot "$P-served"
    Keys '{ESC}' 500
    RightClick $DX $DY 1200
    Shot "$P-inline"
    Keys '{ESC}' 500
  }
  'items' {
    $step = 28   # menu item height (px), measured on the menu shot
    $dl = "$env:USERPROFILE\Downloads"
    Remove-Item "$dl\krunk-256*.png", "$dl\saved-image*.png" -ErrorAction SilentlyContinue
    [System.Windows.Forms.Clipboard]::Clear()
    Focus (MainWindow) | Out-Null
    # Copy image address
    RightClick $SX $SY 1000; Click ($SX + $X) ($SY + $Y + 3 * $step) 1500
    ClipState 'address'
    # Copy image (served)
    RightClick $SX $SY 1000; Click ($SX + $X) ($SY + $Y + 2 * $step) 3000
    ClipState 'served'
    # Copy image (data:)
    RightClick $DX $DY 1000; Click ($DX + $X) ($DY + $Y + 2 * $step) 3000
    ClipState 'inline'
    # Save image as…: the dialog, then a name
    RightClick $SX $SY 1000; Click ($SX + $X) ($SY + $Y + $step) 2500
    Shot "$P-save-dialog"
    $dlg = [U]::FindWindow('#32770', 'Save image as')
    Log "[$P] save dialog: $($dlg -ne [IntPtr]::Zero)"
    if ($dlg -ne [IntPtr]::Zero) {
      [U]::SetForegroundWindow($dlg) | Out-Null; Start-Sleep -Milliseconds 500
      Keys '%n' 300; Keys "$dl\saved-image.png" 200; Keys '{ENTER}' 3000
    }
    Log "[$P] saved: $(Test-Path "$dl\saved-image.png") $((Get-Item "$dl\saved-image.png" -ErrorAction SilentlyContinue).Length) bytes, same as served: $((Get-FileHash "$dl\saved-image.png" -ErrorAction SilentlyContinue).Hash -eq (Get-FileHash "$www\krunk-256.png").Hash)"
    # Save image as… on the data: image
    RightClick $DX $DY 1000; Click ($DX + $X) ($DY + $Y + $step) 2500
    $dlg = [U]::FindWindow('#32770', 'Save image as')
    if ($dlg -ne [IntPtr]::Zero) {
      [U]::SetForegroundWindow($dlg) | Out-Null; Start-Sleep -Milliseconds 500
      Keys '%n' 300; Keys "$dl\saved-image-inline.png" 200; Keys '{ENTER}' 2000
    }
    Log "[$P] inline saved: $(Test-Path "$dl\saved-image-inline.png") $((Get-Item "$dl\saved-image-inline.png" -ErrorAction SilentlyContinue).Length) bytes"
    # Open image in new tab
    Focus (MainWindow) | Out-Null
    RightClick $SX $SY 1000; Click ($SX + $X) ($SY + $Y) 2500
    Shot "$P-new-tab"
    Log "[$P] after open: $(Titles)"
    StopKrunk
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  }
}
