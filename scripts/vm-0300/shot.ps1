# Desktop screenshot into C:\pipeline\krunk\v0300\shots\<Name>.png, plus whether a Windows
# Security (firewall) prompt is up. -Esc closes such a prompt afterwards; -Keys sends keys first.
param([string]$Name = 'x', [switch]$Esc, [string]$Keys = '', [int]$X = -1, [int]$Y = -1, [switch]$Right)
. C:\pipeline\krunk\lib.ps1
$Shots = 'C:\pipeline\krunk\v0300\shots'
if ($X -ge 0) { if ($Right) { RightClick $X $Y 900 } else { Click $X $Y 900 } }
if ($Keys) { foreach ($k in $Keys.Split(' ')) { Keys $k 350 } ; Start-Sleep 1 }
Start-Sleep -Milliseconds 800
Shot $Name
$fw = [U]::FindWindow([NullString]::Value, 'Windows Security')
Log "$Name firewallPrompt=$($fw -ne [IntPtr]::Zero)"
if ($Esc -and $fw -ne [IntPtr]::Zero) { [void][U]::SetForegroundWindow($fw); Start-Sleep -Milliseconds 500; Keys '{ESC}' 1000 }
