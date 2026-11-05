# Krunk 0.4.0 milestone 2: closing a tab animates. Eight tabs; the close button of tab 3 is
# clicked, then the tab row is captured every ~25 ms for 400 ms ("tabanim-burst-NN"), then two
# more clicks on the same spot (the next tabs' close buttons slide under the pointer), then the
# pointer leaves the row (the tabs widen). Fresh profile. Screenshots "tabanim-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'tabanim'
# Captures the tab row (screen rect) $n times, $ms apart, into memory; saves them afterwards.
function Burst([string]$name, [int]$x, [int]$y, [int]$w, [int]$h, [int]$n, [int]$ms) {
  $frames = @()
  $t0 = Get-Date
  for ($i = 0; $i -lt $n; $i++) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $g.Dispose()
    $frames += , @($bmp, [int]((Get-Date) - $t0).TotalMilliseconds)
    Start-Sleep -Milliseconds $ms
  }
  $i = 0
  foreach ($f in $frames) {
    $f[0].Save("$Shots\$P-$name-$('{0:D2}' -f $i)-$($f[1])ms.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $f[0].Dispose(); $i++
  }
}

Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = 'C:\pipeline\krunk\p040a'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
SafeGo 'https://example.com/?1' 1500
foreach ($n in 2..8) { SafeKeys '^t' 900; SafeGo "https://example.com/?$n" 1200 }
SafeKeys '^1' 1500
$c = ClientRect $A
Shot "$P-1-eight"

# The close button of tab 3 (of 8): tab x + width - 18 DIP.
$avail = ($c.Right - $c.Left) / $c.Scale - 222
$w = [Math]::Min(256, $avail / 8)
$cx = [int]($c.Left + (8 + ($w + 2) * 2 + $w - 18) * $c.Scale)
$cy = [int]($c.Top + 18 * $c.Scale)
Log "[close] tab width $([int]$w) DIP; close button at $cx,$cy"
[U]::SetCursorPos($cx, $cy) | Out-Null; AbsMove $cx $cy; Start-Sleep -Milliseconds 400
[SI]::Mouse(2, 0, 0) | Out-Null; [SI]::Mouse(4, 0, 0) | Out-Null
Burst 'burst' $c.Left $c.Top 1100 40 16 25
Start-Sleep -Milliseconds 300
Shot "$P-2-after-one-close"
# Two more clicks on the same spot, 300 ms apart.
[SI]::Mouse(2, 0, 0) | Out-Null; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 300
[SI]::Mouse(2, 0, 0) | Out-Null; [SI]::Mouse(4, 0, 0) | Out-Null; Start-Sleep -Milliseconds 500
Shot "$P-3-three-closed-frozen"
# Leave the row: the tabs take their new widths.
AbsMove $cx ($cy + 300); Start-Sleep -Milliseconds 100
Burst 'thaw' $c.Left $c.Top 1100 40 10 25
Start-Sleep -Milliseconds 300
Shot "$P-4-thawed"
# Ctrl+W (keyboard): no freezing; Ctrl+T: a tab grows in.
SafeKeys '^w' 800
Shot "$P-5-ctrl-w"
SafeKeys '^t' 100
Burst 'open' $c.Left $c.Top 1100 40 10 25
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | ForEach-Object { Log "[log] $_" }
