# Krunk 0.28.0 harness: a slow, slightly uneven human-style drag (GitHub's "Slide right" check).
param([int]$X1 = 524, [int]$Y1 = 575, [int]$X2 = 752, [int]$Y2 = 577)
. C:\pipeline\krunk\lib.ps1
AbsMove ($X1 - 40) ($Y1 + 30); Start-Sleep -Milliseconds 400
for ($i = 1; $i -le 10; $i++) { AbsMove ($X1 - 40 + 4 * $i) ($Y1 + 30 - 3 * $i); Start-Sleep -Milliseconds 25 }
[U]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 180
$n = 45; $r = New-Object System.Random
for ($i = 1; $i -le $n; $i++) {
  $t = $i / $n; $e = 1 - [Math]::Pow(1 - $t, 2.2)
  AbsMove ([int]($X1 + ($X2 - $X1) * $e)) ([int]($Y1 + ($Y2 - $Y1) * $t + $r.Next(-2, 3)))
  Start-Sleep -Milliseconds (12 + $r.Next(0, 20))
}
Start-Sleep -Milliseconds 250
[U]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
