# Krunk 0.8.0 (A, items 1-7): page scrolling under an open <select> dropdown, its datalist and
# date-picker cousins, and Krunk's own menus (three-dot, back/forward), side by side with Chrome 157.
# Krunk 0.8.0 keeps the dropdown open and the page still (the brief); Chrome 157 closes it and
# scrolls (measured in select-ref, 2026-11-09). The test page (www\8094\select.html) puts its state into the title: sy=<scrollY> v=<value>
# k=<last key the page saw> f=<focused element>. For every step the run log has the title, the
# visible popup windows of the browser (class, style, owner, rect) and a screenshot.
#   $Cases: which browsers, in order; $App: the Krunk build (app = the release under test).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'sel080'
$K = 'C:\pipeline\krunk'
$App = "$K\app"
$Cases = 'krunk,chrome'
$NavMenu = 0
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class PW {
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
}
'@
Get-Process Krunk, chrome -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$url = 'http://localhost:8094/select.html'

# Visible popup-style top-level windows of the given processes.
function Popups([string]$proc) {
  $pids = @(Get-Process $proc -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $out = @()
  foreach ($h in [TW]::All()) {
    if ($pids -notcontains [TW]::Pid($h) -or -not [TW]::IsWindowVisible($h)) { continue }
    $st = [PW]::GetWindowLong($h, -16)
    if (($st -band 0x80000000) -eq 0) { continue }
    $o = [PW]::GetWindow($h, 4); $ro = [PW]::GetAncestor($h, 3)
    $r = WindowRect $h
    $out += "$([TW]::Cls($h)) style=0x$('{0:X8}' -f $st) owner=$(if ($o -ne [IntPtr]::Zero) { [TW]::Cls($o) } else { '-' }) rootowner=$([TW]::Cls($ro)) rect=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
  }
  if ($out.Count) { $out -join ' | ' } else { 'none' }
}
function Title([IntPtr]$h) { [TW]::Txt($h) }
function Sy([IntPtr]$h) { $m = [regex]::Match((Title $h), 'sy=(\d+)'); if ($m.Success) { [int]$m.Groups[1].Value } else { -1 } }
function Note([string]$step, [IntPtr]$h, [string]$proc) {
  Log "[$P] $Name $step | title='$(Title $h)' | popups: $(Popups $proc)"
  Shot "$P-$Name-$step"
}
function FgKeys([string]$k, [int]$wait = 400) {
  $cls = [TW]::Cls([U]::GetForegroundWindow())
  if ($cls -notmatch '^(KrunkWindow|KrunkPopup|Chrome_WidgetWin_\d)$') { Log "[$P] foreground '$cls': not typing $k"; return }
  Keys $k $wait
}

function RunCase([IntPtr]$h, [string]$proc, [int]$cx, [int]$cy, [double]$s) {
  # $cx,$cy: the screen point of the page's (0,0); $s: the DPI scale.
  $sel = @([int]($cx + 240 * $s), [int]($cy + 120 * $s))     # the select (40..440 x 60..180)
  $out = @([int]($cx + 1000 * $s), [int]($cy + 520 * $s))    # page area well away from any popup
  $list = @([int]($cx + 200 * $s), [int]($cy + 330 * $s))    # inside the open list (below the select)
  $opt = @([int]($cx + 200 * $s), [int]($cy + 240 * $s))     # an option near the top of the list
  Note '0-base' $h $proc
  # 1. Open, wheel outside, wheel over the list, keys, Esc.
  Click $sel[0] $sel[1] 800
  Note '1-open' $h $proc
  Wheel $out[0] $out[1] 5
  Note '2-wheel-outside' $h $proc
  Wheel $list[0] $list[1] 3
  Note '3-wheel-list' $h $proc
  FgKeys '{DOWN}{DOWN}{DOWN}' 300; FgKeys '{PGDN}' 300; FgKeys 'o' 400
  Note '4-keys' $h $proc
  FgKeys '{ESC}' 600
  Note '5-esc' $h $proc
  $before = Sy $h; Wheel $out[0] $out[1] 3; $after = Sy $h
  Log "[$P] $Name after Esc: wheel 3 notches -> scroll $before -> $after"
  Note '6-after-esc-wheel' $h $proc
  FgKeys '^{HOME}' 600; Click $out[0] $out[1] 300; FgKeys '^{HOME}' 600
  # 2. Choose an option with the mouse, then wheel.
  Click $sel[0] $sel[1] 800
  Wheel $out[0] $out[1] 2
  Note '7-reopen-wheel' $h $proc
  Click $opt[0] $opt[1] 600
  $before = Sy $h; Wheel $out[0] $out[1] 3; $after = Sy $h
  Log "[$P] $Name after choosing: wheel -> scroll $before -> $after"
  Note '8-after-choose-wheel' $h $proc
  FgKeys '^{HOME}' 600
  # 3. Click outside to close, then wheel.
  Click $sel[0] $sel[1] 800
  Click $out[0] $out[1] 600
  $before = Sy $h; Wheel $out[0] $out[1] 3; $after = Sy $h
  Log "[$P] $Name after a click outside: wheel -> scroll $before -> $after"
  Note '9-after-outside-wheel' $h $proc
  FgKeys '^{HOME}' 600
  # 4. Datalist suggestions.
  Click ([int]($cx + 670 * $s)) ([int]($cy + 92 * $s)) 400
  FgKeys 'a' 900
  Note '10-datalist-open' $h $proc
  Wheel $out[0] $out[1] 3
  Note '11-datalist-wheel' $h $proc
  FgKeys '{ESC}' 400; FgKeys '^{HOME}' 600
  # 5. Date picker (the calendar button at the right end of the field).
  Click ([int]($cx + 800 * $s)) ([int]($cy + 160 * $s)) 1200
  Note '12-date-open' $h $proc
  Wheel $out[0] $out[1] 3
  Note '13-date-wheel' $h $proc
  FgKeys '{ESC}' 500; FgKeys '^{HOME}' 600
}

foreach ($Name in $Cases.Split(',')) {
  if ($Name -eq 'krunk') {
    $profile = "$K\p080s"
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    Copy-Item -Recurse "$K\sp-tpl" $profile
    $env:LOCALAPPDATA = $profile
    Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App -ArgumentList $url
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 5
    $A = MainWindow
    [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
    Focus $A | Out-Null
    $c = ClientRect $A
    for ($i = 0; $i -lt 40 -and (Title $A) -notmatch 'sy='; $i++) { Start-Sleep -Milliseconds 250 }
    if ((Title $A) -notmatch 'sy=') { SafeGo $url 2500 }
    Click ([int]($c.Left + 1000 * $c.Scale)) ([int]($c.Top + 400 * $c.Scale)) 300
    Log "[$P] krunk $((Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion) scale $($c.Scale)"
    # The page starts below the shell (76 DIP; the bookmarks bar adds 28 when it is shown).
    $top = 76
    if ((Popups 'Krunk') -ne 'none') { Log "[$P] stray popups at start: $(Popups 'Krunk')" }
    RunCase $A 'Krunk' $c.Left ([int]($c.Top + $top * $c.Scale)) $c.Scale
    # 6. Krunk's own menus: three-dot (and the back/forward menu from 0.8.0 on), wheel over the page.
    $out = @([int]($c.Left + 1000 * $c.Scale), [int]($c.Top + ($top + 520) * $c.Scale))
    Click ([int]($c.Right - 20 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 900
    Note '14-menu-open' $A 'Krunk'
    $before = Sy $A; Wheel $out[0] $out[1] 3; $after = Sy $A
    Log "[$P] krunk three-dot open: wheel over the page -> scroll $before -> $after"
    Note '15-menu-wheel' $A 'Krunk'
    Keys '{ESC}' 600
    if ($NavMenu) {
      SafeGo "$url`?second" 2000; SafeKeys '^{HOME}' 400
      RightClick ([int]($c.Left + 22 * $c.Scale)) ([int]($c.Top + 56 * $c.Scale)) 900
      Note '16-navmenu-open' $A 'Krunk'
      $before = Sy $A; Wheel $out[0] $out[1] 3; $after = Sy $A
      Log "[$P] krunk back menu open: wheel over the page -> scroll $before -> $after"
      Note '17-navmenu-wheel' $A 'Krunk'
      Keys '{ESC}' 600
      $before = Sy $A; Wheel $out[0] $out[1] 3; $after = Sy $A
      Log "[$P] krunk menus closed: wheel -> scroll $before -> $after"
    }
    # Close normally (killing the processes one by one makes the frame log a renderer exit).
    if (Focus $A) { SafeKeys '%{F4}' 2500 }
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep 1
    Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
    Log "[$P] krunk.log WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count)"
  } else {
    $cr = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
    $crp = "$K\cr080"
    Remove-Item -Recurse -Force $crp -ErrorAction SilentlyContinue
    Start-Process -FilePath $cr -ArgumentList @("--user-data-dir=$crp", '--no-first-run', '--no-default-browser-check', '--window-position=0,0', '--window-size=1280,760', $url)
    $A = [IntPtr]::Zero
    for ($i = 0; $i -lt 60 -and $A -eq [IntPtr]::Zero; $i++) {
      Start-Sleep -Milliseconds 500
      $w = Get-Process chrome -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
      if ($w) { $A = $w.MainWindowHandle }
    }
    Start-Sleep 2
    [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
    Focus $A | Out-Null
    $c = ClientRect $A
    for ($i = 0; $i -lt 40 -and (Title $A) -notmatch 'sy='; $i++) { Start-Sleep -Milliseconds 250 }
    Click ([int]($c.Left + 1000 * $c.Scale)) ([int]($c.Top + 400 * $c.Scale)) 300
    Log "[$P] chrome $((Get-Item $cr).VersionInfo.ProductVersion) scale $($c.Scale)"
    # Chrome's tab strip and toolbar: 87 DIP above the page in a restored window.
    RunCase $A 'chrome' $c.Left ([int]($c.Top + 87 * $c.Scale)) $c.Scale
    Click ([int]($c.Right - 20 * $c.Scale)) ([int]($c.Top + 62 * $c.Scale)) 900
    Note '14-menu-open' $A 'chrome'
    $before = Sy $A; Wheel ([int]($c.Left + 600 * $c.Scale)) ([int]($c.Top + 600 * $c.Scale)) 3; $after = Sy $A
    Log "[$P] chrome three-dot open: wheel over the page -> scroll $before -> $after"
    Note '15-menu-wheel' $A 'chrome'
    Keys '{ESC}' 600
    RightClick ([int]($c.Left + 22 * $c.Scale)) ([int]($c.Top + 62 * $c.Scale)) 900
    Note '16-backmenu-open' $A 'chrome'
    Keys '{ESC}' 600
    Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -Force
  }
}
Get-Content "$K\www\access.log" -Tail 200 | Select-String 'sel=' | Select-Object -Last 60 | ForEach-Object {
  $m = [regex]::Match($_.Line, 'sel=([^ ]+)'); Log "[page] $([uri]::UnescapeDataString($m.Groups[1].Value))"
}
Log "[$P] done"
