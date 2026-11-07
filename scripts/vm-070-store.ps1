# Krunk 0.7.0 M9 (R20-R22): extensions from the live Chrome Web Store. Phases (Krunk stays open):
#   open    fresh profile; the Downloads folder listed; Dark Reader's store page ("Add to Krunk")
#   click   the button at $Bx,$By (client DIP): the confirmation popup, then Add
#   ubo     uBlock Origin Lite's store page, its button, the confirmation, Add
#   check   krunk://extensions (light, dark), the Downloads folder again, the log
#   popup   the click-away cases of an extension popup (Esc, page click, tab switch, other window)
param([string]$Phase = 'open', [int]$Bx = 0, [int]$By = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "store-$Phase"
$profile = "$K\p070st"
$env:LOCALAPPDATA = $profile
$dl = "$env:USERPROFILE\Downloads"
function StartKrunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $w = MainWindow
  [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $w | Out-Null
}
function Win { $w = MainWindow; Focus $w | Out-Null; $script:c = ClientRect $w }
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow {
  $pids = KrunkPids
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { WindowRect $pw } else { $null }
}
function PopupCount { $pids = KrunkPids; @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' }).Count }
function LogExt([string]$what) { Log "[$P] $what`: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'extension|Web Store' | Select-Object -Last 8 | ForEach-Object { $_.Line -replace '^\d+ ', '' }) -join ' ## ')" }
switch ($Phase) {
  'open' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    Set-Content "$K\store-downloads-before.txt" ((Get-ChildItem $dl -Name | Sort-Object) -join "`n")
    Log "[$P] Downloads before: $(@(Get-ChildItem $dl).Count) entries"
    StartKrunk; Win
    SafeGo 'https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh' 9000
    Shot "$P-1-dark-reader"
  }
  'click' {
    Win
    Click (X $Bx) (Y $By) 9000
    $r = PopupRectNow
    Log "[$P] confirmation: $(if ($r) { "$($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { 'none' })"
    Shot "$P-1-confirm"
    if ($r) { Keys '{ENTER}' 4000 }
    Shot "$P-2-added"
    LogExt 'after Add'
  }
  'ubo' {
    Win
    SafeGo 'https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh' 9000
    Click (X $Bx) (Y $By) 9000
    $r = PopupRectNow
    Log "[$P] confirmation: $(if ($r) { "$($r.Left),$($r.Top)-$($r.Right),$($r.Bottom)" } else { 'none' })"
    Shot "$P-1-confirm"
    if ($r) { Keys '{ENTER}' 5000 }
    Shot "$P-2-added"
    LogExt 'after Add'
  }
  'check' {
    Win
    SafeGo 'krunk://extensions' 3000
    Shot "$P-1-extensions"
    $after = (Get-ChildItem $dl -Name | Sort-Object) -join "`n"
    $before = Get-Content "$K\store-downloads-before.txt" -Raw
    Log "[$P] Downloads folder unchanged: $(($before.Trim()) -eq ($after.Trim()))"
    LogExt 'log'
  }
  'popup' {
    # Dark Reader's toolbar button (the first extension button) at $Bx,$By (screen pixels).
    Win
    SafeGo 'https://example.com/' 3000
    function Open { Click $Bx $By 3500; PopupCount }
    $n = Open; Shot "$P-1-open"
    $r = PopupRectNow
    if ($r) { Click ([int](($r.Left + $r.Right) / 2)) ([int]($r.Bottom - 40)) 1200 }
    Log "[$P] click inside the popup: popups $n -> $(PopupCount)"
    Shot "$P-1b-after-inside-click"
    Keys '{ESC}' 1200; Log "[$P] Esc: popups $n -> $(PopupCount)"
    $n = Open; Click (X 400) (Y 500) 1200; Log "[$P] click on the page: popups $n -> $(PopupCount)"
    $n = Open; SafeKeys '^{TAB}' 1200; Log "[$P] tab switch: popups $n -> $(PopupCount)"
    SafeKeys '^{TAB}' 800
    $n = Open; Click $Bx $By 1500; Log "[$P] its own button again: popups $n -> $(PopupCount)"
    $n = Open; Click 640 778 1500; Log "[$P] click on the taskbar (another window): popups $n -> $(PopupCount)"
    Win
    Click (X 400) (Y 500) 800                      # back into the window first
    $n = Open; Click (X 300) (Y 56) 1200; Log "[$P] click in the address bar: popups $n -> $(PopupCount)"
    Keys '{ESC}' 300
    $n = Open; Log "[$P] reopened after all that: popups $n"
    Shot "$P-2-reopened"
    Keys '{ESC}' 600
  }
}
Log "[$P] done"
