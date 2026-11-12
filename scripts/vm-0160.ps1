# Krunk 0.16.0: the VM pass, one phase per run (scripts/vm-0160-run.sh PHASE). The build in
# C:\pipeline\krunk\<App>, a fresh profile per phase, the window at 0,0 1280x760 at 100 %.
# Test pages: www\8094 (scripts/vm-www/8094). The AI phases need the mock provider on
# 127.0.0.1:18111 (ui/dev/mock-provider.cjs behind `ssh -R`, started by vm-0160-run.sh).
#   menus     ⋮ → Session usage; the Home button (off by default, on by setting, click,
#             middle-click, Ctrl+click, empty home page, narrow window)
#   ext       the fixture extension installed from its zip; right-click its toolbar icon: the
#             menu, its details page, Options, Disable, Uninstall… (Cancel, then Remove)
#   private   private tab titles on eight looks, light and dark: inactive, hovered, active, and
#             with the window in the background
#   aimenu    right-click in the AI panel: the message box, selected answer text, a link
#   scratch   the agent's scratchpad with the mock provider; the marker is searched on disk
#             while Krunk runs and after it has exited
#   password  fill_password: consent, generated password on signup.html, the card, the save
#             prompt; a provided password on login.html; the refusal without a request
param([string]$Phase = 'menus', [string]$App = 'app', [string]$Marker = 'KRUNKMARK0160A')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$Root = 'C:\pipeline\krunk'
$W = 'http://127.0.0.1:8094'
$Py = 'C:\Program Files\Python312\python.exe'
$P = "v0160-$Phase"
Remove-Item "$Shots\$P-*" -Force -ErrorAction SilentlyContinue
$script:n = 0
function Snap([string]$name) { $script:n++; Shot ("$P-{0:d2}-$name" -f $script:n); Log "[$P] shot $name" }
function Note([string]$s) { Log "[$P] $s" }
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 1
}
function StartKrunk([string]$prof) {
  $env:LOCALAPPDATA = $prof
  Start-Process -FilePath "$Root\$App\Krunk.exe" -WorkingDirectory "$Root\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $script:A = MainWindow
  for ($i = 0; $i -lt 40 -and [TW]::Txt($script:A) -notmatch 'Krunk'; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  for ($i = 0; $i -lt 4 -and -not (Focus $script:A); $i++) { Start-Sleep 1 }
  if ([U]::GetForegroundWindow() -ne $script:A) { Note 'could not focus'; throw 'no focus' }
  $script:c = ClientRect $script:A
}
function SetKv([string]$prof, [string]$patch) {
  [IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
  $out = "$(& $Py "$Root\setkv.py" $prof "@$prof\patch.json" 2>&1)"
  if ($out -match 'Traceback') { Start-Sleep 5; $out = "$(& $Py "$Root\setkv.py" $prof "@$prof\patch.json" 2>&1)" }
  Note "setkv: $($out.Substring(0, [Math]::Min(300, $out.Length)))"
}
# A new profile: one start creates the database, then the settings patch.
function Fresh([string]$prof, [string]$patch) {
  StopKrunk
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $prof | Out-Null
  StartKrunk $prof
  StopKrunk
  SetKv $prof $patch
}
# Loads a page; $expect must show in the window title (typed keys can get lost): up to 3 tries.
function Page([string]$url, [int]$wait = 2500, [string]$expect = '') {
  foreach ($try in 1..3) {
    Focus $script:A | Out-Null; SafeGo $url $wait
    if ($expect -eq '') { return }
    for ($i = 0; $i -lt 10 -and [TW]::Txt($script:A) -notlike "*$expect*"; $i++) { Start-Sleep -Milliseconds 500 }
    if ([TW]::Txt($script:A) -like "*$expect*") { return }
    Note "page ${url}: title is '$([TW]::Txt($script:A))' after try $try"
    Keys '{ESC}' 300
  }
  throw "page $url did not load"
}
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function XR([double]$dip) { [int]($c.Right - $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function Title { [TW]::Txt($script:A) }
function KLog([string]$prof, [string]$pattern) {
  (Get-Content "$prof\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join "`n"
}
# Krunk's open popup window (menus, prompts), or $null.
function PopupNow {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if ($pw) { WindowRect $pw } else { $null }
}
function PopupNote([string]$label) {
  $r = PopupNow
  if ($r) { Note "$label popup rect=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom) ($($r.Right - $r.Left)x$($r.Bottom - $r.Top))" } else { Note "$label popup: none" }
}
function MiddleClick([int]$x, [int]$y, [int]$wait = 500) {
  MoveTo $x $y; [U]::mouse_event(0x0020, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [U]::mouse_event(0x0040, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function CtrlClick([int]$x, [int]$y, [int]$wait = 500) {
  [U]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  Click $x $y 100
  [U]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds $wait
}
function Tabs { (KLog $script:prof 'tab [0-9]+: (created|closed)' ).Split("`n").Count }
function Serve {
  if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-File', "$Root\serve.ps1", '-PrefixList', 'http://*:8094/'
    Start-Sleep 2
  }
}
$base = '"bookmarksBar": "never", "checkUpdatesAutomatically": false, "theme": "light", "startup": "newTab"'
$ai = '"ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"}'
# Types into the AI panel's message box (the panel is open, docked right, 380 DIP wide).
function Ask([string]$text, [int]$wait) {
  Focus $script:A | Out-Null
  Click ($c.Right - 190) ($c.Bottom - 26) 300
  SafeKeys $text 150
  SafeKeys '{ENTER}' $wait
}

try {
  Serve
  if ($Phase -eq 'menus') {
    $script:prof = "$Root\p0160m"
    # --- default: no Home button; ⋮ has Session usage ---
    Fresh $prof "{ $base, `"homepage`": `"$W/c2.html`" }"
    StartKrunk $prof
    Page "$W/c1.html" 2500 'Blue'
    Snap 'default-no-home'
    Click (XR 22) (Y 56) 1200
    Snap 'menu'; PopupNote 'menu'
    $m = PopupNow
    # Rows after the padding: account, separator, four tab items, separator, History,
    # Bookmarks, Downloads, Passwords, Cookies, then Session usage. 30 px rows and 11 px
    # separators; in a 760 px window the menu is higher than the room under its button and
    # gets its compact rows (24 px, separators 5 px, 4 px padding; 0.15.4).
    if ($m) {
      $dense = ($m.Bottom - $m.Top) -lt 600
      $y = if ($dense) { 4 + 24 + 5 + 4 * 24 + 5 + 5 * 24 + 12 } else { 6 + 30 + 11 + 4 * 30 + 11 + 5 * 30 + 15 }
      Note "menu rows are $(if ($dense) { 'compact' } else { 'normal' }); Session usage at +$y"
      Click ($m.Left + 80) ($m.Top + $y) 2500
    }
    Snap 'session-usage-page'
    Note "after Session usage: title '$(Title)'"
    StopKrunk
    # --- the setting on ---
    SetKv $prof '{"appearance": {"showHomeButton": true}}'
    StartKrunk $prof
    Page "$W/c1.html" 2500 'Blue'
    Snap 'home-button'
    # Home sits after Reload: Back 22, Forward 55, Reload 90, Home about 124 DIP.
    Click (X 124) (Y 56) 2500
    Note "after Home click: title '$(Title)'"
    Snap 'home-clicked'
    Page "$W/c1.html" 2500 'Blue'
    MiddleClick (X 124) (Y 56) 2500
    Note "after Home middle-click: title '$(Title)'"
    Snap 'home-middle-click'
    $t = TabPoint $script:A 0 2; Click $t[0] $t[1] 1200
    CtrlClick (X 124) (Y 56) 2500
    Note "after Home Ctrl+click: title '$(Title)'"
    Snap 'home-ctrl-click'
    # A second window has the button too.
    SafeKeys '^n' 3500
    Snap 'home-second-window'
    SafeKeys '%{F4}' 2000
    Focus $script:A | Out-Null
    # The setting's row in Settings → Appearance (search finds it).
    Page 'krunk://settings#appearance' 3000 'Settings'
    Snap 'settings-appearance'
    # Narrow window: Home goes with downloads / VPN / AI; Back, Forward, Reload, shield, ⋮ stay.
    [U]::MoveWindow($script:A, 0, 0, 480, 400, $true) | Out-Null; Start-Sleep 2
    Snap 'narrow'
    [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
    StopKrunk
    # --- an empty home page is the new-tab page ---
    SetKv $prof '{"homepage": ""}'
    StartKrunk $prof
    Page "$W/c1.html" 2500 'Blue'
    Click (X 124) (Y 56) 2500
    Note "empty home page, after Home click: title '$(Title)'"
    Snap 'home-empty-is-newtab'
    Note "log:`n$(KLog $prof 'WARN|ERROR')"
    StopKrunk
  }

  if ($Phase -eq 'ext') {
    $script:prof = "$Root\p0160e"
    Fresh $prof "{ $base }"
    StartKrunk $prof
    Page 'krunk://extensions' 3000 'Extensions'
    Snap 'extensions-empty'
    # "Install from file…" (0.11.x layout at 1280 wide).
    Click 904 144 2500
    Keys 'C:\pipeline\krunk\vm-ext\krunk-fixture.zip' 300; Keys '{ENTER}' 6000
    Snap 'install-prompt'
    $r = PopupNow
    if ($r) { Click ($r.Right - 66) ($r.Bottom - 30) 4000 } else { Note 'no install prompt'; Keys '{ENTER}' 4000 }
    Focus $script:A | Out-Null
    Snap 'installed'
    Page "$W/c1.html" 2500 'Blue'
    Snap 'toolbar-icon'
    # The extension's icon is left of the shield (shield at 55 DIP from the right when the AI
    # button is hidden): about 90 DIP from the right edge.
    $ix = XR 90; $iy = Y 56
    RightClick $ix $iy 1200
    Snap 'ext-menu'; PopupNote 'ext-menu'
    $m = PopupNow
    if (-not $m) { Note 'no extension menu: wrong icon position?'; throw 'no ext menu' }
    # Rows of 30 px after 6 px: name, [separator 11], Options, Manage extensions, Disable, [separator], Uninstall…
    Click ($m.Left + 80) ($m.Top + 6 + 15) 3000
    Note "name item: title '$(Title)'"
    Snap 'ext-details'
    Page "$W/c1.html" 2500 'Blue'
    RightClick $ix $iy 1200
    $m = PopupNow
    if ($m) { Click ($m.Left + 80) ($m.Top + 6 + 30 + 11 + 15) 3000 }
    Note "Options: title '$(Title)'"
    Snap 'ext-options'
    SafeKeys '^w' 800
    RightClick $ix $iy 1200
    $m = PopupNow
    if ($m) { Click ($m.Left + 80) ($m.Top + 6 + 30 + 11 + 30 + 15) 3000 }
    Note "Manage extensions: title '$(Title)'"
    Snap 'ext-manage'
    Page "$W/c1.html" 2500 'Blue'
    # Uninstall…: the second step, Esc, then Cancel, then Remove.
    RightClick $ix $iy 1200
    $m = PopupNow
    if ($m) { Click ($m.Left + 80) ($m.Bottom - 6 - 15) 1200 }
    Snap 'ext-uninstall-ask'; PopupNote 'ext-uninstall-ask'
    Keys '{ESC}' 800
    Snap 'ext-uninstall-esc'
    Note "after Esc: extension folder exists: $(@(Get-ChildItem "$prof\Krunk\extensions" -Directory -ErrorAction SilentlyContinue).Count) folder(s)"
    # Disable from the menu: the icon goes.
    RightClick $ix $iy 1200
    $m = PopupNow
    if ($m) { Click ($m.Left + 80) ($m.Top + 6 + 30 + 11 + 60 + 15) 2500 }
    Snap 'ext-disabled'
    Note "after Disable: $(KLog $prof 'extension' | Select-Object -Last 1)"
    # Enable it again on the page, then uninstall from the menu.
    Page 'krunk://extensions' 3000 'Extensions'
    Snap 'extensions-disabled-card'
    Note "log:`n$(KLog $prof 'extension|WARN|ERROR')"
    StopKrunk
  }

  if ($Phase -eq 'private') {
    $script:prof = "$Root\p0160p"
    Fresh $prof "{ $base }"
    $looks = @(
      @('classic', '"appearance": {"preset": "classic"}'),
      @('sunset', '"appearance": {"preset": "sunset"}'),
      @('aurora', '"appearance": {"preset": "aurora"}'),
      @('candy', '"appearance": {"preset": "candy"}'),
      @('midnight', '"appearance": {"preset": "midnight"}'),
      @('ocean', '"appearance": {"preset": "ocean"}'),
      @('ember', '"appearance": {"preset": "ember"}'),
      @('glass-purple', '"appearance": {"preset": "glass", "accent": "#7c3aed"}'),
      @('strip-purple', '"appearance": {"preset": "classic", "stripColor": "#6d28d9"}')
    )
    foreach ($theme in @('light', 'dark')) {
      foreach ($look in $looks) {
        StopKrunk
        Note "look $($look[0]) $theme"
        # The style's own values come from the settings page's preset logic; here the patch
        # is applied through the AI-free path: the preset name plus what the agent would set.
        & $Py "$Root\preset0160.py" $prof $look[0] $theme 2>&1 | ForEach-Object { Note "preset: $_" }
        StartKrunk $prof
        Page "$W/c1.html" 2500 'Blue'
        SafeKeys '^+n' 2500; SafeGo "$W/c2.html" 2000
        SafeKeys '^+n' 2500; SafeGo "$W/c3.html" 2000
        # Tabs: normal, private (inactive), private (active). Hover the inactive private one.
        $t1 = TabPoint $script:A 1 3
        MoveTo ($t1[0] + 60) ($t1[1] + 300); Start-Sleep -Milliseconds 400
        Snap "$($look[0])-$theme"
        MoveTo ($t1[0] + 60) $t1[1]; Start-Sleep -Milliseconds 700
        Snap "$($look[0])-$theme-hover"
        MoveTo ($t1[0] + 60) ($t1[1] + 300)
        if ($look[0] -in @('classic', 'midnight', 'sunset')) {
          # The window in the background (Notepad in front).
          $t0 = Get-Date
          Start-Process notepad; Start-Sleep 2
          Snap "$($look[0])-$theme-background"
          Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $t0 } | Stop-Process -Force
          Focus $script:A | Out-Null
        }
      }
    }
    StopKrunk
  }

  if ($Phase -eq 'aimenu') {
    $script:prof = "$Root\p0160a"
    Fresh $prof "{ $base, $ai }"
    StartKrunk $prof
    Page "$W/agent.html" 2500
    SafeKeys '^+ ' 1800
    Snap 'panel'
    # The message box: type, select all, right-click.
    Click ($c.Right - 190) ($c.Bottom - 26) 300
    SafeKeys 'some text to cut and copy' 300
    SafeKeys '^a' 300
    RightClick ($c.Right - 190) ($c.Bottom - 26) 1200
    Snap 'menu-on-input'
    Keys '{ESC}' 500
    # An answer with a link: ask, then right-click the answer text and the link.
    SafeKeys '{DEL}' 200
    Ask 'list the links' 9000
    Snap 'answer'
    Note "agent log:`n$(KLog $prof 'agent')"
    StopKrunk
  }

  if ($Phase -eq 'scratch') {
    $script:prof = "$Root\p0160s"
    Fresh $prof "{ $base, $ai }"
    StartKrunk $prof
    Page "$W/agent.html" 2500
    SafeKeys '^+ ' 1800
    Ask "scratchpad test $Marker" 12000
    Snap 'scratch-run'
    # The notepad button in the panel header.
    Note "agent log:`n$(KLog $prof 'agent')"
    # While Krunk runs: the marker must be nowhere on disk.
    $hits = @(Get-ChildItem -Recurse -File "$prof" -ErrorAction SilentlyContinue | Select-String -SimpleMatch $Marker -List -ErrorAction SilentlyContinue | ForEach-Object { $_.Path })
    $hits16 = @(Get-ChildItem -Recurse -File "$prof" -ErrorAction SilentlyContinue | Select-String -SimpleMatch $Marker -Encoding Unicode -List -ErrorAction SilentlyContinue | ForEach-Object { $_.Path })
    Note "marker while running: $($hits.Count) file(s) UTF-8, $($hits16.Count) file(s) UTF-16 $(($hits + $hits16) -join ', ')"
    Ask 'scratchpad limit' 12000
    Snap 'scratch-limit'
    StopKrunk
    $files = @(Get-ChildItem -Recurse -File "$prof" -ErrorAction SilentlyContinue)
    $hits = @($files | Select-String -SimpleMatch $Marker -List -ErrorAction SilentlyContinue | ForEach-Object { $_.Path })
    $hits16 = @($files | Select-String -SimpleMatch $Marker -Encoding Unicode -List -ErrorAction SilentlyContinue | ForEach-Object { $_.Path })
    $temp = @(Get-ChildItem -Recurse -File $env:TEMP -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -gt (Get-Date).AddMinutes(-10) } | Select-String -SimpleMatch $Marker -List -ErrorAction SilentlyContinue | ForEach-Object { $_.Path })
    Note "marker after exit: $($hits.Count) UTF-8, $($hits16.Count) UTF-16 in $($files.Count) profile files; $($temp.Count) in recent %TEMP% files $(($hits + $hits16 + $temp) -join ', ')"
    Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
  }

  if ($Phase -eq 'password') {
    $script:prof = "$Root\p0160w"
    Fresh $prof "{ $base, $ai }"
    StartKrunk $prof
    Page "$W/signup.html" 2500
    SafeKeys '^+ ' 1800
    Ask 'create an account with a generated password' 8000
    Snap 'consent'
    Note "agent log:`n$(KLog $prof 'agent')"
    Copy-Item "$prof\Krunk\krunk.log" "$Shots\$P-krunk.log"
    StopKrunk
  }
} catch {
  Note "ERROR $_"
} finally {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
}
