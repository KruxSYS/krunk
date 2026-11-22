# Krunk 0.27.0 drag-out check: a finished download dragged from krunk://downloads (-From page)
# or from the toolbar's downloads popup (-From popup) onto an Explorer window lands in that
# folder. Phase "prep": fresh lp-base profile, downloads dragtest-<Tag>.txt from httpbin, opens
# the page or popup, shot dragout-<Tag>-prep. Phase "drag": Explorer on dragdst at the right,
# (-BtnX,-BtnY: the downloads button is clicked first, shot dragout-<Tag>-popup) drags from (-X,-Y) onto it, shot dragout-<Tag>-after, run.log says where the file is.
param([string]$App = 'app271', [string]$Phase = 'prep', [string]$From = 'page', [string]$Tag = 'a', [int]$X = 0, [int]$Y = 0, [int]$BtnX = 0, [int]$BtnY = 0)
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'
$Dst = "$K\dragdst"
$Name = "response-headers.txt"  # httpbin names the file itself
if ($Phase -eq 'prep') {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
  $P = "lp-drag-$Tag"
  Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$P"
  Remove-Item -Recurse -Force $Dst -ErrorAction SilentlyContinue; New-Item -ItemType Directory $Dst | Out-Null
  Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'dragtest-*' -ErrorAction SilentlyContinue | Remove-Item -Force
  $env:LOCALAPPDATA = "$K\$P"
  Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $A = MainWindow; [U]::MoveWindow($A, 0, 0, 760, 760, $true) | Out-Null; Start-Sleep 6
  [U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
  [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 800
  # An unpublished build of a published version gets the Repair popup: Later, back to the frame.
  Click 627 193 800; Click 380 30 300
  Go "https://httpbin.org/response-headers?Content-Type=text/plain&Content-Disposition=attachment%3B%20filename%3D$Name" 6000
  if ($From -eq 'page') { Go 'krunk://downloads' 2500 } else { Keys '{ESC}' 300 }
  Shot "dragout-$Tag-prep"
  Log "prep: downloads has $((Get-ChildItem "$env:USERPROFILE\Downloads" -Filter $Name).Count) $Name"
} else {
  Start-Process explorer.exe $Dst; Start-Sleep 3
  $ex = (New-Object -ComObject Shell.Application).Windows() | Where-Object { $_.LocationURL -like '*dragdst*' } | Select-Object -First 1
  if ($ex) { [U]::MoveWindow([IntPtr]$ex.HWND, 760, 0, 520, 760, $true) | Out-Null }
  Start-Sleep 1
  $A = MainWindow
  [U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [void][U]::SetForegroundWindow($A)
  [U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 600
  Click 607 193 600
  if ($BtnX) { Click $BtnX $BtnY 1500; Shot "dragout-$Tag-popup" }
  Drag $X $Y 1000 400 30
  Start-Sleep 2
  Shot "dragout-$Tag-after"
  Log "drag: in dragdst: $((Get-ChildItem $Dst -Name) -join ',') | still in Downloads: $(Test-Path "$env:USERPROFILE\Downloads\$Name")"
  if ($ex) { $ex.Quit() }
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
}
