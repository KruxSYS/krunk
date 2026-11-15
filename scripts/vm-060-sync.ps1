# Android 0.6.0 (M5): the Windows side of the sync end-to-end tests: Krunk's --dev build (staging
# through the ssh -R tunnel on 127.0.0.1:18080) in a fresh profile, as the fixture account's
# Windows device. Phases are run one by one from Linux (scripts/vm.sh ps1 with $Phase set).
#   start      fresh profile p060a, Krunk started, krunk://settings#account
#   shot       a screenshot only
#   go URL     navigate the active tab ($Arg)
#   click X Y  page coordinates ($Arg = "x y")
#   keys K     SendKeys ($Arg)
#   text       the page's text into page.txt (secrets never logged)
#   stop       close Krunk
param([string]$Phase = 'shot', [string]$Arg = '')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$App = "$K\appdev060"
$profile = if ($env:KRUNK_P) { $env:KRUNK_P } elseif (Test-Path "$K\use-p060s") { "$K\p060s" } else { "$K\p060a" }
$P = "v060-$Phase"
function Win { $script:A = MainWindow; Focus $A | Out-Null; $script:c = ClientRect $A }
function PageClick([int]$x, [int]$y, [int]$wait = 1500) { Click ([int]($c.Left + $x * $c.Scale)) ([int]($c.Top + (104 + $y) * $c.Scale)) $wait }
function PageTextOnly { Set-Clipboard -Value 'empty'; SafeKeys '^a' 300; SafeKeys '^c' 400; $t = (Get-Clipboard) -join "`n"; Set-Clipboard -Value 'cleared'; $t }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  [U]::MoveWindow((MainWindow), 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Win
}
switch ($Phase) {
  'start' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
    if ($Arg -ne 'keep') { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$profile\Krunk" | Out-Null }
    StartKrunk
    SafeGo $(if ($Arg -eq 'keep') { 'krunk://sync' } else { 'krunk://settings#account' }) 3000
    Shot $P
  }
  'shot' { Win; Shot "$P-$Arg" }
  'go' { Win; SafeGo $Arg 3000; Shot "$P" }
  'click' { Win; $xy = $Arg -split ' '; PageClick ([int]$xy[0]) ([int]$xy[1]) 2500; Shot "$P" }
  'keys' { Win; SafeKeys $Arg 1500; Shot "$P" }
  'type' { Win; [System.Windows.Forms.SendKeys]::SendWait(($Arg -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep 1 }
  'text' { Win; Set-Content -Encoding utf8 "$K\page.txt" (PageTextOnly) }
  'log' { Set-Content -Encoding utf8 "$K\klog.txt" (KLog $Arg) }
  'stop' {
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 4; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  }
}
# rclick X Y: right click at screen coordinates (the bookmarks bar is above the page)
if ($Phase -eq 'rclick') { Win; $xy = $Arg -split ' '; RightClick ([int]$xy[0]) ([int]$xy[1]) 1200; Shot "v060-rclick" }
# sclick X Y: left click at screen coordinates (popups, the bookmarks bar)
if ($Phase -eq 'sclick') { $xy = $Arg -split ' '; Click ([int]$xy[0]) ([int]$xy[1]) 1500; Shot "v060-sclick" }
