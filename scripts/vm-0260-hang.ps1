# Krunk 0.26.0 hang diagnosis: start -App on a fresh copy of lp-base (real network) with
# KRUNK_PERF_OFF=-Off, wait 20 s, type an address, and log whether the window hangs (IsHungAppWindow)
# and its title. Krunk stays running (cdb attaches from the host); -Step stop closes it.
param([string]$Step = 'start', [string]$App = 'app260rel', [string]$Off = '', [string]$Tag = 'rel')
. C:\pipeline\krunk\lib.ps1
Add-Type -Namespace H -Name W -MemberDefinition '[DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);' -ErrorAction SilentlyContinue
$K = 'C:\pipeline\krunk'
if ($Step -eq 'stop') { Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Log "[hang] stopped"; return }
Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
$P = "lp-hang-$Tag"
Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$P"
$env:LOCALAPPDATA = "$K\$P"; $env:KRUNK_PERF_OFF = $Off
$t0 = Get-Date
Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow
Start-Sleep 20
[void][U]::SetForegroundWindow($A); Start-Sleep -Milliseconds 500
Go 'https://en.wikipedia.org/wiki/Web_browser' 10000
$sb = New-Object System.Text.StringBuilder 256; [void][H.W]::GetWindowText($A, $sb, 256)
$hung = [H.W]::IsHungAppWindow($A)
Shot "hang-$Tag"
$lines = (Get-Content "$K\$P\Krunk\krunk.log" -ErrorAction SilentlyContinue | Measure-Object -Line).Lines
$pre = (Select-String -Path "$K\$P\Krunk\krunk.log" -Pattern 'blocked host\(s\) handed|filter lists' -ErrorAction SilentlyContinue | % { $_.Line }) -join ' | '
Log "[hang $Tag] off='$Off' hung=$hung title='$($sb.ToString())' log_lines=$lines t=$([int]((Get-Date)-$t0).TotalSeconds)s :: $pre"
