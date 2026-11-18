param(
  [int]$N = 20,
  [string]$Tag = 'n20',
  [string]$App = 'C:\pipeline\krunk\app',
  [string]$Phases = 'switch,rapid,far,discard,open,close,drag,cpu,flash',
  [string]$Theme = 'dark',
  [switch]$Trace,
  [string]$PerfLog = '',
  [int]$SleepTest = 10,
  [int]$Settle = 15,
  [int]$StripLeft = 6,
  [int]$StripReserve = 222,
  [int]$PageTop = 0
)
# Krunk 0.25.0 (M2): the tab switch / open / close / drag measurement pass.
# Built from vm-0100-tabs.ps1 (N tabs restored from a saved session, High priority harness,
# sshd AboveNormal, report-serve.py on 8097) and vm-0111-drag.ps1 (C# SendInput driver with QPC
# logging, framecap start/stop).
#
# Tabs: tab i is heavy.html (even i) or busy.html (odd i) on localhost:8097 with ?m=i (the marker
# band, marker.js), every 3rd (i % 3 == 2) with a muted video, tab 5 with a 3 MB DOM (heavy). Every
# phase starts Krunk afresh (fresh profile, the session restored, maximized; one pass of Ctrl+Tab
# loads every tab, Ctrl+1, -Settle s) and stops it afterwards, so each phase has its own
# krunk-<phase>.log and perf-<phase>.log.
#
# Phases (-Phases, comma list, in order):
#   switch   25 x Ctrl+Tab, 700 ms apart
#   rapid    50 x Ctrl+Tab every 150 ms, then 20 clicks across the visible strip ~250 ms apart
#   far      10 clicks on tabs not visited for >= 60 s (computed strip positions)
#   discard  --sleep-test-seconds=$SleepTest with tab discarding on; once background tabs are
#            discarded: 5 x Ctrl+<digit> onto discarded (non-video) tabs, then Ctrl+9 and
#            3 x Ctrl+W onto discarded neighbours. Recorded over the page area (flash check too)
#   open     10 x (Ctrl+T, Ctrl+W) from the last tab; then 10 x (link -> new tab, Ctrl+W) from
#            newtabs.html (whose link opens heavy.html?m=N+1+k)
#   close    10 x Ctrl+W onto loaded neighbours (from tab 0)
#   drag     strip reorder (a tab across 10 slots, maximized, strip recording), then at 700x500
#            a window move by the title bar and a tear-off (moves + full-screen recordings for
#            draglag.py --marker; drag-markers.txt names each recording's marker)
#   cpu      a new tab page in front, 30 s of \Process(krunk*)\% Processor Time
#   flash    page-area recording: 3 x (Ctrl+T, Ctrl+W), 3 x Ctrl+W onto a loaded tab,
#            3 x loading heavy.html?dom=1 in the active tab
# Output in shots\v0250-<Tag>\: <rec>-events.csv (qpf,<freq>; qpc,kind,target) next to the
# framecap recording <rec>.raw/.csv/.json, drag-*-moves.csv, cpu.txt, krunk-/perf-<phase>.log,
# reports.txt, run.log, <phase>.done. Targets are marker indices (-1: any change).
# -Trace sets KRUNK_PERF_TRACE=1; -PerfLog names the trace file (default: perf.log next to
# krunk.log in the profile). -StripLeft/-StripReserve tune the computed tab positions (DIP).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "v0250-$Tag"
$Out = "$Shots\$P"
New-Item -ItemType Directory -Force $Out | Out-Null
$Prof = "$K\p0250"
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'High'
Get-Process sshd -ErrorAction SilentlyContinue | ForEach-Object { try { $_.PriorityClass = 'AboveNormal' } catch {} }
$Cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
$NoBom = New-Object Text.UTF8Encoding($false)

Add-Type @"
using System; using System.Diagnostics; using System.Threading; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class D25 {
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint p);
  [DllImport("winmm.dll")] static extern uint timeEndPeriod(uint p);
  [StructLayout(LayoutKind.Sequential)] public struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] public struct IN { [FieldOffset(0)] public uint type; [FieldOffset(8)] public MI mi; [FieldOffset(8)] public KI ki; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  // Every logged key or click: "qpc,kind,target" (QPC at the key's or button's down).
  public static List<string> Events = new List<string>();
  // Every mouse move of a drag: "qpc,x,y".
  public static List<string> Moves = new List<string>();
  static void Mouse(int dx, int dy, uint flags) { var i = new IN[1]; i[0].type = 0; i[0].mi.dx = dx; i[0].mi.dy = dy; i[0].mi.flags = flags; SendInput(1, i, Marshal.SizeOf(typeof(IN))); }
  static void Kb(ushort vk, bool up) { var i = new IN[1]; i[0].type = 1; i[0].ki.vk = vk; i[0].ki.flags = up ? 2u : 0u; SendInput(1, i, Marshal.SizeOf(typeof(IN))); }
  static void Log(long q, string kind, int target) { if (kind.Length > 0) Events.Add(q + "," + kind + "," + target); }
  public static void To(int x, int y) {
    int l = GetSystemMetrics(76), t = GetSystemMetrics(77), w = GetSystemMetrics(78), h = GetSystemMetrics(79);
    Mouse((int)((x - l) * 65535.0 / (w - 1)), (int)((y - t) * 65535.0 / (h - 1)), 0x8001 | 0x4000);
  }
  // Ctrl+vk (ctrl = false: vk alone); logs the QPC of vk's down.
  public static long Key(ushort vk, bool ctrl, string kind, int target) {
    if (ctrl) { Kb(0x11, false); Thread.Sleep(15); }
    long q = Stopwatch.GetTimestamp(); Kb(vk, false); Thread.Sleep(15); Kb(vk, true);
    if (ctrl) { Thread.Sleep(10); Kb(0x11, true); }
    Log(q, kind, target); return q;
  }
  public static long Click(int x, int y, int settleMs, string kind, int target) {
    To(x, y); Thread.Sleep(settleMs);
    long q = Stopwatch.GetTimestamp(); Mouse(0, 0, 0x0002); Thread.Sleep(40); Mouse(0, 0, 0x0004);
    Log(q, kind, target); return q;
  }
  public static void Down() { Mouse(0, 0, 0x0002); }
  public static void Up() { Mouse(0, 0, 0x0004); }
  // n moves of (dx, dy) from (x, y), one every ms milliseconds, each logged.
  public static string Path(int x, int y, int dx, int dy, int n, int ms) {
    timeBeginPeriod(1);
    var sw = Stopwatch.StartNew(); double next = 0; int k = 0;
    while (k < n) {
      if (sw.Elapsed.TotalMilliseconds >= next) { k++; To(x + dx * k, y + dy * k); Moves.Add(Stopwatch.GetTimestamp() + "," + (x + dx * k) + "," + (y + dy * k)); next += ms; }
      Thread.Sleep(0);
    }
    timeEndPeriod(1);
    return n + " moves in " + Math.Round(sw.Elapsed.TotalMilliseconds) + " ms";
  }
}
"@

function Note([string]$line) { Log "[$P] $line"; Add-Content "$Out\run.log" "$(Get-Date -Format HH:mm:ss.fff) $line" }
function WriteLines([string]$file, $lines) { [IO.File]::WriteAllLines($file, [string[]]@($lines), $NoBom) }

# The marker palette (the same list the pages use).
$Pal = @([regex]::Matches((Get-Content "$K\www\8094\marker.js" -Raw), "'#([0-9a-fA-F]{6})'") | ForEach-Object { $_.Groups[1].Value })
function MarkerRgb([int]$m) {
  $hx = $Pal[$m % $Pal.Count]
  @([Convert]::ToInt32($hx.Substring(0, 2), 16), [Convert]::ToInt32($hx.Substring(2, 2), 16), [Convert]::ToInt32($hx.Substring(4, 2), 16))
}

# ---------------- the pages ----------------
$Base = 'http://localhost:8097'
$RepLog = "$K\reports-0250.log"
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'report-[s]erve\.py' })) {
  Start-Process python -WindowStyle Hidden -ArgumentList "$K\report-serve.py", "$K\www\8094", '8097', $RepLog
  Start-Sleep 2
}
Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'report-[s]erve\.py' } |
  ForEach-Object { try { (Get-Process -Id $_.ProcessId).PriorityClass = 'High' } catch {} }
$RepMark = (Get-Item $RepLog -ErrorAction SilentlyContinue).Length
if (-not $RepMark) { $RepMark = 0 }
function TabUrl([int]$i) {
  $page = if ($i % 2 -eq 0) { 'heavy.html' } else { 'busy.html' }
  $extra = ''
  if ($i % 3 -eq 2) { $extra += '&video=1' }
  if ($i -eq 5) { $page = 'heavy.html'; $extra += '&dom=1' }
  "$Base/$page`?m=$i&tag=$Tag-t$i$extra"
}
function IsVideo([int]$m) { ($m -lt $N) -and ($m % 3 -eq 2) }

# ---------------- Krunk ----------------
$A = [IntPtr]::Zero
$CR = $null
$order = New-Object System.Collections.ArrayList
$act = 0
$openers = @{}

function StartKrunk([string]$mode) {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 1500
  Remove-Item -Recurse -Force $Prof -ErrorAction SilentlyContinue
  Copy-Item -Recurse "$K\sp-tpl" $Prof
  $tabs = @(0..($N - 1) | ForEach-Object { @{ url = (TabUrl $_); title = "tab $_" } })
  $session = @{ version = 2; tabs = $tabs; active = 0; windows = @(@{ tabs = $tabs; active = 0; maximized = $true }); focused = 0 }
  # (PowerShell 5's -Encoding utf8 writes a BOM, which Python's json refuses.)
  [IO.File]::WriteAllText("$K\session-0250.json", ($session | ConvertTo-Json -Depth 6 -Compress), $NoBom)
  $r = python $K\setkv.py $Prof "@$K\session-0250.json" session 2>&1
  Note "session: tabs in it: $(([regex]::Matches([string]$r, '"url"')).Count)"
  $settings = @{ checkUpdatesAutomatically = $false; startup = 'restoreSession'; theme = $Theme; tabDiscardEnabled = ($mode -eq 'discard') }
  [IO.File]::WriteAllText("$K\settings-0250.json", ($settings | ConvertTo-Json -Compress), $NoBom)
  Note "settings: $(python $K\setkv.py $Prof "@$K\settings-0250.json")"
  $env:LOCALAPPDATA = $Prof
  if ($Trace) { $env:KRUNK_PERF_TRACE = '1' } else { $env:KRUNK_PERF_TRACE = '' }
  if ($mode -eq 'discard') {
    Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App -ArgumentList "--sleep-test-seconds=$SleepTest"
  } else {
    Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  }
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::ShowWindow($A, 3) | Out-Null
  Start-Sleep 1
  if (-not (Focus $A)) { Note 'could not focus Krunk'; throw 'no focus' }
  $script:order = New-Object System.Collections.ArrayList
  for ($i = 0; $i -lt $N; $i++) { [void]$order.Add($i) }
  $script:act = 0
  $script:openers = @{}
  # One pass through every tab loads the restored ones, then back to tab 0.
  for ($i = 1; $i -lt $N; $i++) { Guard; [void][D25]::Key(0x09, $true, '', 0); Start-Sleep -Milliseconds 350 }
  Guard; [void][D25]::Key(0x31, $true, '', 0)
  $script:CR = ClientRect $A
  [D25]::To($CR.Right - 60, $CR.Bottom - 60)    # the cursor away from the strip (hover cards)
  Note "Krunk $((Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion) ($mode): $N tabs, client $($CR.Left),$($CR.Top),$($CR.Right),$($CR.Bottom) scale $($CR.Scale)"
  Start-Sleep $Settle
}

function StopKrunk([string]$name) {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  Copy-Item "$Prof\Krunk\krunk.log" "$Out\krunk-$name.log" -ErrorAction SilentlyContinue
  $pl = if ($PerfLog) { $PerfLog } else { "$Prof\Krunk\perf.log" }
  if (Test-Path $pl) { Copy-Item $pl "$Out\perf-$name.log" } elseif ($Trace) { Note "no trace at $pl" }
}

# Typing only into Krunk: refocus the main window once, else stop the phase.
function Guard {
  if ([TW]::Cls([U]::GetForegroundWindow()) -eq 'KrunkWindow') { return }
  if (-not (Focus $A)) { Note "focus lost (foreground '$([TW]::Cls([U]::GetForegroundWindow()))')"; throw 'focus lost' }
}

# ---------------- the tab model (marker per strip position) ----------------
function Target { if ($order.Count -eq 0) { -1 } else { [int]$order[$act] } }
function SwitchNext([string]$kind, [int]$waitMs) {
  $script:act = ($act + 1) % $order.Count
  Guard; [void][D25]::Key(0x09, $true, $kind, (Target)); Start-Sleep -Milliseconds $waitMs
}
function Digit([int]$pos, [string]$kind, [int]$waitMs) {     # Ctrl+1..8 = position 0..7, Ctrl+9 = last
  if ($pos -ge 8 -or $pos -ge $order.Count) { $pos = $order.Count - 1; $vk = 0x39 } else { $vk = 0x31 + $pos }
  $script:act = $pos
  Guard; [void][D25]::Key($vk, $true, $kind, (Target)); Start-Sleep -Milliseconds $waitMs
}
function NewTab([string]$kind, [int]$waitMs) {
  [void]$order.Add(-1); $script:act = $order.Count - 1
  Guard; [void][D25]::Key(0x54, $true, $kind, -1); Start-Sleep -Milliseconds $waitMs
}
# Krunk (core/src/tabs.rs close): the opener if still there, else the tab now at the index, else
# the one before.
function CloseTab([string]$kind, [int]$waitMs) {
  $gone = [int]$order[$act]
  $order.RemoveAt($act)
  if ($gone -ge 0 -and $openers.ContainsKey($gone) -and $order.Contains($openers[$gone])) { $script:act = $order.IndexOf($openers[$gone]) }
  elseif ($order.Count -gt 0) { $script:act = [Math]::Min($act, $order.Count - 1) }
  Guard; [void][D25]::Key(0x57, $true, $kind, (Target)); Start-Sleep -Milliseconds $waitMs
  if ($order.Count -eq 0) { [void]$order.Add(-1); $script:act = 0 }
}

# Tab positions in the strip (shell.css / shell.js): tabs prefer 256 DIP and shrink evenly; when
# they are narrower than 100 the active one keeps 120 (narrow mode); below 72 the row scrolls
# and gets ‹ › and ▾ (26 + 26 + 34 DIP more). Assumes the row scrolled to its start (Ctrl+1).
function StripLayout {
  $c = ClientRect $A
  $avail = ($c.Right - $c.Left) / $c.Scale - $StripReserve
  $cnt = $order.Count
  $nat = ($avail - 2 * ($cnt - 1)) / [Math]::Max(1, $cnt)
  $x0 = $StripLeft; $over = $false
  if ($nat -ge 100) { $w = [Math]::Min(256, $nat); $wa = $w }
  else {
    $wa = 120
    $w = if ($cnt -gt 1) { ($avail - 2 * ($cnt - 1) - 120) / ($cnt - 1) } else { 120 }
    if ($w -lt 72) { $w = 72; $over = $true; $x0 += 26; $avail -= 86 }
  }
  [pscustomobject]@{ C = $c; W = $w; WA = $wa; X0 = $x0; Over = $over; Avail = $avail }
}
function TabCenter([int]$pos) {
  $lay = StripLayout
  $left = $lay.X0
  for ($i = 0; $i -lt $pos; $i++) { $left += $(if ($i -eq $act) { $lay.WA } else { $lay.W }) + 2 }
  $wp = if ($pos -eq $act) { $lay.WA } else { $lay.W }
  @([int]($lay.C.Left + ($left + $wp / 2) * $lay.C.Scale), [int]($lay.C.Top + 21 * $lay.C.Scale))
}
# Positions fully visible with the row at its start (clear of the right fade).
function VisibleTabs {
  $lay = StripLayout
  if (-not $lay.Over) { return $order.Count }
  [Math]::Max(1, [int][Math]::Floor(($lay.Avail - 28 - 120) / ($lay.W + 2)) + 1)
}
function ClickTab([int]$pos, [string]$kind, [int]$settleMs, [int]$waitMs) {
  $pt = TabCenter $pos
  $script:act = $pos
  Guard; [void][D25]::Click($pt[0], $pt[1], $settleMs, $kind, (Target)); Start-Sleep -Milliseconds $waitMs
}

# ---------------- recordings ----------------
# The page's top edge: the first row (down the client area's middle column) in the active tab's
# marker colour.
function FindPageTop {
  if ($PageTop -gt 0) { return $PageTop }
  $c = ClientRect $A
  $x = [int](($c.Left + $c.Right) / 2); $hh = [int](300 * $c.Scale)
  $bmp = New-Object System.Drawing.Bitmap 1, $hh
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $c.Top, 0, 0, $bmp.Size); $g.Dispose()
  $rgb = MarkerRgb (Target)
  $top = 0
  for ($y = 0; $y -lt $hh; $y++) {
    $px = $bmp.GetPixel(0, $y)
    if ([Math]::Abs($px.R - $rgb[0]) -le 24 -and [Math]::Abs($px.G - $rgb[1]) -le 24 -and [Math]::Abs($px.B - $rgb[2]) -le 24) { $top = $c.Top + $y; break }
  }
  $bmp.Dispose()
  if ($top -eq 0) { $top = [int]($c.Top + 112 * $c.Scale); Note "marker band not found: page top guessed at $top" }
  $top
}
function BandRect { $c = ClientRect $A; $t = FindPageTop; "$($c.Left),$t,$($c.Right - $c.Left),$([int](20 * $c.Scale))" }
function PageRect { $c = ClientRect $A; $t = FindPageTop; "$($c.Left),$t,$($c.Right - $c.Left),$($c.Bottom - $t)" }
function ScreenRect { "0,0,$([D25]::GetSystemMetrics(0)),$([D25]::GetSystemMetrics(1))" }
function StartCap([string]$name, [string]$rect, [int]$step, [int]$secs) {
  [D25]::Events.Clear(); [D25]::Moves.Clear()
  Note "capture $name rect $rect step $step for $secs s"
  $proc = Start-Process -FilePath "$K\tools\framecap.exe" -ArgumentList '--rect', $rect, '--step', "$step", '--seconds', "$secs", '--out', "$Out\$name" -WindowStyle Hidden -PassThru
  Start-Sleep -Milliseconds 800
  $proc
}
function EndCap($proc, [string]$name) {
  $proc.WaitForExit(120000) | Out-Null
  $q = [Diagnostics.Stopwatch]::Frequency
  if ([D25]::Events.Count -gt 0) { WriteLines "$Out\$name-events.csv" (@("qpf,$q") + [D25]::Events) }
  if ([D25]::Moves.Count -gt 0) { WriteLines "$Out\$name-moves.csv" (@("qpc,x,y,qpf=$q") + [D25]::Moves) }
  Note "capture $name done: $([D25]::Events.Count) events, $([D25]::Moves.Count) moves; $(Get-Content "$Out\$name.json" -ErrorAction SilentlyContinue)"
}

# ---------------- phases ----------------
function Phase-Switch {
  $cap = StartCap 'switch' (BandRect) 4 22
  for ($i = 0; $i -lt 25; $i++) { SwitchNext 'switch' 700 }
  EndCap $cap 'switch'
}
function Phase-Rapid {
  $vis = [Math]::Min((VisibleTabs), $order.Count)
  $cap = StartCap 'rapid' (BandRect) 4 20
  for ($i = 0; $i -lt 50; $i++) { SwitchNext 'rapid' 150 }
  Start-Sleep -Milliseconds 1000
  if ($vis -ge 2) {
    for ($i = 0; $i -lt 20; $i++) { ClickTab (($i * 5 + 1) % $vis) 'rapidclick' 30 180 }
  }
  EndCap $cap 'rapid'
  Note "rapid: clicks over $vis visible tabs"
}
function Phase-Far {
  # The load pass visited every tab before -Settle; wait out the rest of the 60 s.
  Start-Sleep ([Math]::Max(0, 60 - $Settle + 5))
  $vis = [Math]::Min((VisibleTabs), $order.Count)
  $picks = @(@(9, 2, 7, 4, 11, 1, 8, 3, 10, 5, 12, 6) | Where-Object { $_ -lt $vis - 1 } | Select-Object -First 10)
  Note "far: $vis visible tabs, clicking positions $($picks -join ' ')"
  if ($picks.Count -eq 0) { Note 'far: no far tabs (N too small)'; return }
  $cap = StartCap 'far' (BandRect) 4 ([int]($picks.Count * 1.6) + 4)
  foreach ($pos in $picks) { ClickTab $pos 'far' 120 1400 }
  EndCap $cap 'far'
}
function Phase-Discard {
  # Background tabs discard after 4 x -SleepTest s (power.rs); video tabs may stay awake.
  $want = @(1..([Math]::Max(1, $N - 1)) | Where-Object { $_ -lt $N -and -not (IsVideo $_) }).Count
  $gone = 0
  for ($i = 0; $i -lt 40; $i++) {
    $gone = @(Select-String -Path "$Prof\Krunk\krunk.log" -Pattern ': discarded' -ErrorAction SilentlyContinue).Count
    if ($gone -ge $want) { break }
    Start-Sleep 5
  }
  Note "discard: $gone tabs discarded (waited for $want)"
  $picks = @(@(1, 3, 4, 6, 7) | Where-Object { $_ -lt $N })
  $cap = StartCap 'discard' (PageRect) 8 ([int](($picks.Count + 4) * 2.6) + 5)
  foreach ($pos in $picks) { Digit $pos 'discard' 2500 }
  # The last tab (not visited since the load pass), then closes onto its discarded neighbours.
  if ($N -ge 12) {
    Digit 99 'discard' 2500
    for ($i = 0; $i -lt 3; $i++) {
      $next = [int]$order[$act - 1]
      CloseTab $(if (IsVideo $next) { 'closevideo' } else { 'closedisc' }) 2500
    }
  }
  EndCap $cap 'discard'
}
function Phase-Open {
  if ($N -gt 1) { Digit 99 '' 1500 }
  $cap = StartCap 'open-ntp' (BandRect) 4 33
  for ($i = 0; $i -lt 10; $i++) { NewTab 'open' 1400; CloseTab 'closenew' 1400 }
  EndCap $cap 'open-ntp'
  # The link page in a new tab of its own (marker N), its links open markers N+1...
  NewTab '' 1200
  SafeGo "$Base/newtabs.html?m=$N&base=$($N + 1)&tag=$Tag-nt" 2500
  $order[$act] = $N
  $c = ClientRect $A
  $cx = [int](($c.Left + $c.Right) / 2); $cy = [int](($c.Top + $c.Bottom) / 2 + 60 * $c.Scale)
  $cap = StartCap 'open-link' (BandRect) 4 42
  for ($i = 0; $i -lt 10; $i++) {
    $m = $N + 1 + $i
    $opener = [int]$order[$act]
    $order.Insert($act + 1, $m); $openers[$m] = $opener; $script:act = $act + 1
    Guard; [void][D25]::Click($cx, $cy, 120, 'link', $m); Start-Sleep -Milliseconds 1900
    CloseTab 'closelink' 1400
  }
  EndCap $cap 'open-link'
}
function Phase-Close {
  $cnt = [Math]::Min(10, $N - 1)
  if ($cnt -lt 1) { Note 'close: needs 2 tabs'; return }
  $cap = StartCap 'close' (BandRect) 4 ([int]($cnt * 1.5) + 4)
  for ($i = 0; $i -lt $cnt; $i++) { CloseTab 'close' 1500 }
  EndCap $cap 'close'
}
function Phase-Drag {
  $scr = ScreenRect
  $marks = @()
  # 1: reorder in the maximized window: tab 1 across up to 10 slots (strip recording).
  if ($N -ge 3) {
    Digit 1 '' 900
    $slots = [Math]::Min(10, [Math]::Min((VisibleTabs), $N) - 2)
    $from = TabCenter 1; $to = TabCenter (1 + $slots)
    $c = ClientRect $A
    $cap = StartCap 'drag-reorder' "$($c.Left),$($c.Top),$($c.Right - $c.Left),$([int](36 * $c.Scale))" 2 5
    [D25]::To($from[0], $from[1]); Start-Sleep -Milliseconds 150
    [D25]::Down(); Start-Sleep -Milliseconds 200
    $r3 = [D25]::Path($from[0], $from[1], [int](($to[0] - $from[0]) / 60), 0, 60, 8)
    Start-Sleep -Milliseconds 300
    [D25]::Up()
    EndCap $cap 'drag-reorder'
    $m = [int]$order[1]; $order.RemoveAt(1); $order.Insert(1 + $slots, $m); $script:act = 1 + $slots
    Note "drag-reorder: marker $m across $slots slots: $r3"
    Start-Sleep -Milliseconds 800
    Shot "$P-drag-1-reordered"
  }
  # 2: the window (700 x 500 at 20,20) by its title bar (the strip's empty space left of the
  #    caption buttons): 96 px down (draglag's "tear-off" run), then 140 moves of 3,1 px / 8 ms.
  [U]::ShowWindow($A, 9) | Out-Null; Start-Sleep -Milliseconds 500
  [U]::MoveWindow($A, 20, 20, 700, 500, $true) | Out-Null; Start-Sleep 1
  Guard
  $c = ClientRect $A; $sc = $c.Scale
  $x = [int]($c.Right - 160 * $sc); $y = [int]($c.Top + 18 * $sc)
  $marks += "drag-move $(Target)"
  $cap = StartCap 'drag-move' $scr 4 6
  [D25]::To($x, $y); Start-Sleep -Milliseconds 150
  [D25]::Down(); Start-Sleep -Milliseconds 500        # the modal move loop starts late
  $r1 = [D25]::Path($x, $y, 0, 8, 12, 8)
  $r2 = [D25]::Path($x, $y + 96, 3, 1, 140, 8)
  Start-Sleep -Milliseconds 300
  [D25]::Up()
  EndCap $cap 'drag-move'
  Note "drag-move: $r1; $r2; marker $(Target)"
  Start-Sleep -Milliseconds 800
  Shot "$P-drag-2-moved"
  # 3: tear-off of the active tab (window back at 20,20): 96 px down out of the strip, then 140
  #    moves of 3,1 px. In the narrow window the active tab may sit past the visible strip: tab 0.
  if ($N -ge 2) {
    [U]::MoveWindow($A, 20, 20, 700, 500, $true) | Out-Null
    Start-Sleep 4                                         # 0.11.1: the spare frame comes 3 s after load
    if ($act -ge (VisibleTabs) - 1) { Digit 0 '' 900 }
    $t = TabCenter $act
    $cap = StartCap 'drag-tear' $scr 4 6
    [D25]::To($t[0], $t[1]); Start-Sleep -Milliseconds 150
    [D25]::Down(); Start-Sleep -Milliseconds 200
    $r4 = [D25]::Path($t[0], $t[1], 0, 8, 12, 8)
    $r5 = [D25]::Path($t[0], $t[1] + 96, 3, 1, 140, 8)
    Start-Sleep -Milliseconds 300
    [D25]::Up()
    EndCap $cap 'drag-tear'
    Note "drag-tear: $r4; $r5; marker $(Target)"
    $marks += "drag-tear $(Target)"
    Start-Sleep -Milliseconds 800
    Shot "$P-drag-3-torn"
    KrunkWindows "$P-drag"
  }
  # Which marker each recording follows (for draglag.py --marker).
  WriteLines "$Out\drag-markers.txt" $marks
}
function Phase-Cpu {
  NewTab '' 3000        # every busy tab in the background
  $smp = Get-Counter '\Process(krunk*)\% Processor Time' -SampleInterval 2 -MaxSamples 15
  $per = @($smp | ForEach-Object { ($_.CounterSamples | Measure-Object CookedValue -Sum).Sum / $Cores })
  $avg = ($per | Measure-Object -Average).Average; $max = ($per | Measure-Object -Maximum).Maximum
  $line = 'cpu krunk_avg_pct={0:N1} krunk_max_pct={1:N1} cores={2} samples={3} tabs={4}' -f $avg, $max, $Cores, $per.Count, $N
  WriteLines "$Out\cpu.txt" @($line)
  Note $line
}
function Phase-Flash {
  $cap = StartCap 'flash' (PageRect) 8 32
  for ($i = 0; $i -lt 3; $i++) { NewTab 'open' 1500; CloseTab 'closenew' 1500 }
  for ($i = 0; $i -lt 3 -and $order.Count -gt 1; $i++) { CloseTab 'close' 1500 }
  for ($i = 0; $i -lt 3; $i++) {
    $m = $N + 20 + $i
    SafeKeys '^l' 200; SafeKeys "$Base/heavy.html?m=$m&dom=1&tag=$Tag-load$i" 150
    $order[$act] = $m
    Guard; [void][D25]::Key(0x0D, $false, 'load', $m); Start-Sleep -Milliseconds 2600
  }
  EndCap $cap 'flash'
}

Note "N=$N tag=$Tag app=$App phases=$Phases theme=$Theme trace=$([bool]$Trace) screen $(ScreenRect)"
foreach ($ph in ($Phases -split ',')) {
  $ph = $ph.Trim()
  if (-not $ph) { continue }
  $t0 = Get-Date
  try {
    $mode = 'normal'
    if ($ph -eq 'discard') { $mode = 'discard' }
    StartKrunk $mode
    switch ($ph) {
      'switch' { Phase-Switch }
      'rapid' { Phase-Rapid }
      'far' { Phase-Far }
      'discard' { Phase-Discard }
      'open' { Phase-Open }
      'close' { Phase-Close }
      'drag' { Phase-Drag }
      'cpu' { Phase-Cpu }
      'flash' { Phase-Flash }
      default { Note "unknown phase $ph" }
    }
  } catch {
    Note "phase $ph failed: $_"
    Shot "$P-$ph-error"
  } finally {
    StopKrunk $ph
    Note "phase ${ph}: $([int]((Get-Date) - $t0).TotalSeconds) s"
    WriteLines "$Out\$ph.done" @('done')
  }
}
# The pages' own reports from this run.
if (Test-Path $RepLog) {
  $fs = [System.IO.File]::Open($RepLog, 'Open', 'Read', 'ReadWrite'); $fs.Seek($RepMark, 'Begin') | Out-Null
  $text = (New-Object System.IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
  WriteLines "$Out\reports.txt" @($text -split "`n" | Where-Object { $_ -match " $Tag-" } | ForEach-Object { $_.TrimEnd() })
}
Note 'done'
