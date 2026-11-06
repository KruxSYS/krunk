# Krunk 0.5.0 milestone 2: the shell quick fixes. (a) The Home button is gone. (b) Three clicks on
# the menu button (and the shield) alternate open, closed, open: the press that closes the popup
# must not reopen it. (c) Opening a bookmark from the bar while the caret is in the address bar
# moves the focus to the page and shows the page's address. (d) The previous update download in
# %TEMP%\Krunk-update is removed by the retry 30 s after start, even when it was in use at start.
# Fresh profile. Screenshots "shell-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'shell'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class GT {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
    public int cbSize; public int flags; public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner, hwndMoveSize, hwndCaret; public RECT rcCaret; }
  [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO g);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  // The focused window of the foreground thread and its screen rect.
  public static string Focus() {
    var g = new GUITHREADINFO(); g.cbSize = Marshal.SizeOf(typeof(GUITHREADINFO));
    if (!GetGUIThreadInfo(0, ref g)) return "no gui thread info";
    RECT r; GetWindowRect(g.hwndFocus, out r);
    return "focus=0x" + g.hwndFocus.ToString("x") + " rect=" + r.L + "," + r.T + "," + r.R + "," + r.B;
  }
}
'@
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
# (d) A leftover update download that is still in use when Krunk starts.
$upd = Join-Path $env:TEMP 'Krunk-update'
New-Item -ItemType Directory -Force $upd | Out-Null
Set-Content (Join-Path $upd 'Krunk-Setup-old.exe') 'x'
$hold = [IO.File]::Open((Join-Path $upd 'Krunk-Setup-old.exe'), 'Open', 'Read', 'None')
Log "[temp] $upd exists before start, file held open: $(Test-Path $upd)"

$profile = 'C:\pipeline\krunk\p050s'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$started = Get-Date
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
Log "[temp] after start (held): $(Test-Path $upd)"
$hold.Close()
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
$toolbarY = [int]($c.Top + 56 * $c.Scale)
$barY = [int]($c.Top + 90 * $c.Scale)

# (a) Toolbar without the Home button.
SafeGo 'https://example.com/' 4000
SafeKeys '^d' 1500   # bookmark it (onto the bar)
Shot "$P-1-toolbar-no-home"

# (b) Menu: open, closed, open, closed with four clicks.
$menuX = [int]($c.Right - 22 * $c.Scale)
$shieldX = [int]($c.Right - 56 * $c.Scale)
function Popups { @([TW]::All() | Where-Object { [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' }).Count }
Click $menuX $toolbarY 900; Log "[menu] click 1: popup windows visible = $(Popups)"; Shot "$P-2-menu-click1-open"
Click $menuX $toolbarY 900; Log "[menu] click 2: popup windows visible = $(Popups)"; Shot "$P-3-menu-click2-closed"
Click $menuX $toolbarY 900; Log "[menu] click 3: popup windows visible = $(Popups)"; Shot "$P-4-menu-click3-open"
Click $menuX $toolbarY 900; Log "[menu] click 4: popup windows visible = $(Popups)"; Shot "$P-5-menu-click4-closed"
Click $shieldX $toolbarY 900; Log "[shield] click 1: popup windows visible = $(Popups)"; Shot "$P-6-shield-click1-open"
Click $shieldX $toolbarY 900; Log "[shield] click 2: popup windows visible = $(Popups)"; Shot "$P-7-shield-click2-closed"
Click $shieldX $toolbarY 900; Log "[shield] click 3: popup windows visible = $(Popups)"; Shot "$P-8-shield-click3-open"
Keys '{ESC}' 600

# (c) New tab (caret in the address bar), then click the bookmark on the bar.
SafeKeys '^t' 2000
Log "[focus] on the new tab: $([GT]::Focus()) (shell view rect expected, top of the client area)"
Shot "$P-9-newtab-caret"
$itemX = [int]($c.Left + 60 * $c.Scale)
Click $itemX $barY 3500
Log "[focus] after the bookmark click: $([GT]::Focus()) (page view rect expected, below the bar)"
Shot "$P-10-bookmark-opened"
# Typing now must go to the page, not the address bar: Ctrl+F in the page would open the find bar
# of the shell either way, so type a character and check the address bar did not change.
Keys 'q' 500
Shot "$P-11-typed-into-page"

# (d) The retry removed the update folder 30 s after start.
$left = 36 - ((Get-Date) - $started).TotalSeconds
if ($left -gt 0) { Start-Sleep ([int]$left + 1) }
Log "[temp] 36 s after start: $(Test-Path $upd) (expected False)"

KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'update:|WARN|ERROR' | ForEach-Object { Log "[log] $($_.Line)" }
