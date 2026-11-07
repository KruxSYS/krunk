# Krunk 0.6.0 milestone 11 (R22): password generation. A fresh profile gets a vault; on the
# sign-up page (www/8094/signup.html) the focused new-password field shows "Use a strong
# password" in the dropdown; the pick fills both password fields; "Create account" saves the
# login at once with the "Password saved · Undo · Edit" toast; Undo removes it; a second round
# keeps it and Edit opens the entry on krunk://passwords; a private tab offers no generator.
param([string]$Phase = 'main')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "pg-$Phase"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060g'
$master = 'Krunk-test-master-1'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function PopupRectNow {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1
  if (-not $pw) { return $null }
  WindowRect $pw
}
function Quit { foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }; Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1; Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue }
# (1) The vault (the 0.5 autofill script's coordinates).
SafeGo 'krunk://passwords' 3500
Click 640 371 300; Keys $master 200
Click 640 438 300; Keys $master 200
Click 456 551 4000
Shot "$P-1-vault"
# (2) The sign-up form: the password field shows the generator.
SafeGo 'http://localhost:8094/signup.html' 3500
Click (X 170) (Y 237) 300; Keys 'newuser' 300
Click (X 170) (Y 315) 1500
$pr = PopupRectNow; Log "[$P] dropdown: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"
Shot "$P-2-generator-offered"
if ($pr) { Click ($pr.Left + 120) ($pr.Top + 24) 1500 }
Shot "$P-3-both-filled"
Keys '{ENTER}' 3000
$pr = PopupRectNow; Log "[$P] toast: $(if ($pr) { "$($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)" } else { 'none' })"
Shot "$P-4-saved-toast"
if ($pr) { Click ($pr.Left + 206) ($pr.Top + 57) 1500 }             # Undo (the buttons sit right-aligned: Undo, Edit, OK)
SafeGo 'krunk://passwords' 3000
Shot "$P-5-undone"
# (3) Again, kept this time; Edit opens the entry.
SafeGo 'http://localhost:8094/signup.html' 3500
Click (X 170) (Y 237) 300; Keys 'newuser' 300
Click (X 170) (Y 315) 1500
$pr = PopupRectNow
if ($pr) { Click ($pr.Left + 120) ($pr.Top + 24) 1500 }
Keys '{ENTER}' 3000
$pr = PopupRectNow
Shot "$P-6-saved-again"
if ($pr) { Click ($pr.Left + 268) ($pr.Top + 57) 3000 }             # Edit
Shot "$P-7-edit-opens-entry"
# (4) A private tab: no generator.
SafeKeys '^+n' 2500; SafeKeys 'http://localhost:8094/signup.html' 100; SafeKeys '{ENTER}' 3500
Click (X 170) (Y 315) 1500
$pr = PopupRectNow; Log "[$P] private dropdown: $(if ($pr) { 'shown' } else { 'none' })"
Shot "$P-8-private-no-generator"
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'generated|login saved|login updated|automatic|suggestions') -join ' ## ')"
Log "[$P] warnings: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'WARN|ERROR' | Select-Object -Last 8) -join ' ## ')"
Quit
Log "[$P] done"
