# Frameless-window checks on the VM (run through scripts/vm.sh ps1 after copying lib.ps1):
# drag, double-click maximize/restore, edge and corner resize, caption buttons, Snap Layouts
# flyout, system menu. Positions are computed from the client area and DPI, so it also
# runs at 150 %. Writes run.log and screenshots with the prefix $Prefix.
param([string]$Prefix = 'm2')
. C:\pipeline\krunk\lib.ps1
$h = MainWindow
[U]::ShowWindow($h, 9) | Out-Null; Start-Sleep 0.5
$wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
[U]::MoveWindow($h, $wa.Left + 100, $wa.Top + 60, [int]($wa.Width * 0.7), [int]($wa.Height * 0.75), $true) | Out-Null; Start-Sleep 1.5
$c = ClientRect $h; $s = $c.Scale
Log "[$Prefix] scale $s client $($c.Left),$($c.Top),$($c.Right),$($c.Bottom) workarea $($wa.Left),$($wa.Top),$($wa.Right),$($wa.Bottom)"
Shot "$Prefix-01-restored"
function DragSpot($c) { @([int]($c.Right - (138 + 20) * $c.Scale), [int]($c.Top + 18 * $c.Scale)) }
$d = DragSpot $c
Drag $d[0] $d[1] ($d[0] + 120) ($d[1] + 80)
$c2 = ClientRect $h; Log "drag +120,+80: client moved by $($c2.Left - $c.Left),$($c2.Top - $c.Top)"
$d = DragSpot $c2
DoubleClick $d[0] $d[1]; Start-Sleep 1
$r = WindowRect $h; $cm = ClientRect $h
Log "double-click: zoomed=$([U]::IsZoomed($h)) window $($r.Left),$($r.Top),$($r.Right),$($r.Bottom) client $($cm.Left),$($cm.Top),$($cm.Right),$($cm.Bottom) (client must equal the work area)"
Shot "$Prefix-02-maximized"
$d = DragSpot $cm
DoubleClick $d[0] $d[1]; Start-Sleep 1
Log "double-click again: zoomed=$([U]::IsZoomed($h))"
$r = WindowRect $h; $my = [int](($r.Top + $r.Bottom) / 2); $c3 = ClientRect $h
Drag ($r.Right - [int](4 * $s)) $my ($r.Right - [int](4 * $s) - 100) $my
$c4 = ClientRect $h; Log "right edge -100: width $($c3.Right - $c3.Left) -> $($c4.Right - $c4.Left)"
$r = WindowRect $h
Drag ($r.Right - [int](4 * $s)) ($r.Bottom - [int](4 * $s)) ($r.Right - [int](4 * $s) - 60) ($r.Bottom - [int](4 * $s) - 40)
$c5 = ClientRect $h; Log "corner -60,-40: size $($c4.Right - $c4.Left)x$($c4.Bottom - $c4.Top) -> $($c5.Right - $c5.Left)x$($c5.Bottom - $c5.Top)"
$mx = [int](($c5.Left + $c5.Right) / 2)
Drag $mx ($c5.Top + [int](2 * $s)) $mx ($c5.Top + [int](2 * $s) + 40)
$c6 = ClientRect $h; Log "top edge +40: top $($c5.Top) -> $($c6.Top), height $($c5.Bottom - $c5.Top) -> $($c6.Bottom - $c6.Top)"
$r = WindowRect $h
Drag ($r.Left + [int](4 * $s)) $my ($r.Left + [int](4 * $s) + 50) $my
$c7 = ClientRect $h; Log "left edge +50: left $($c6.Left) -> $($c7.Left)"
# Snap Layouts: hover the maximize button
$maxX = [int]($c7.Right - 69 * $s); $capY = [int]($c7.Top + 16 * $s)
for ($i = 12; $i -ge 0; $i--) { AbsMove ($maxX - 3 * $i) $capY; Start-Sleep -Milliseconds 40 }
Start-Sleep 2
Shot "$Prefix-03-snap-layouts"
$closeX = [int]($c7.Right - 23 * $s)
for ($i = 10; $i -ge 0; $i--) { AbsMove ($closeX - 3 * $i) $capY; Start-Sleep -Milliseconds 40 }
Start-Sleep 0.6
Shot "$Prefix-04-close-hover"
Click $maxX $capY 1200
Log "maximize button: zoomed=$([U]::IsZoomed($h))"
$cm = ClientRect $h
Click ([int]($cm.Right - 69 * $s)) ([int]($cm.Top + 16 * $s)) 1200
Log "restore button: zoomed=$([U]::IsZoomed($h))"
$c8 = ClientRect $h
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class Ic { [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h); }'
Click ([int]($c8.Right - 115 * $s)) ([int]($c8.Top + 16 * $s)) 1200
Log "minimize button: iconic=$([Ic]::IsIconic($h))"
[U]::ShowWindow($h, 9) | Out-Null; Start-Sleep 1
$c9 = ClientRect $h
$d = DragSpot $c9
RightClick $d[0] $d[1] 800
Shot "$Prefix-05-system-menu-rightclick"
Keys '{ESC}' 300
Click ([int](($c9.Left + $c9.Right) / 2)) ([int](($c9.Top + $c9.Bottom) / 2)) 400
Keys '% ' 800
Shot "$Prefix-06-system-menu-alt-space"
Keys '{ESC}' 300
