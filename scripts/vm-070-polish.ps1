# Krunk 0.7.0 M8 (R16-R19): links on Krunk's pages, the permissions page's padding, the login
# suggestion popup's bottom, the task manager's selected row; light and dark.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$profile = "$K\p070pol"
$env:LOCALAPPDATA = $profile
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
function StartKrunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $w = MainWindow
  [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $w | Out-Null
  $script:c = ClientRect $w
}
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
foreach ($theme in 'light', 'dark') {
  $P = "polish-$theme"
  if ($theme -eq 'dark') {
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
    Set-Content "$K\pol-patch.json" '{"theme": "dark"}'
    Log "[$P] $(python "$K\setkv.py" $profile "@$K\pol-patch.json" 2>&1)"
  }
  StartKrunk
  if ($theme -eq 'light') {
    # A vault with a login for the test page (for the suggestion popup).
    SafeGo 'krunk://passwords' 2500
    TypeSecret 'polish test master'; Keys '{TAB}' 200; TypeSecret 'polish test master'; Keys '{ENTER}' 4000
    Click 825 145 800
    TypeSecret 'http://localhost:8094'; Keys '{TAB}' 100; TypeSecret 'alice@example.org'; Keys '{TAB}' 100; TypeSecret 'Tr0ub4dor-3'; Keys '{ENTER}' 1500
  } else {
    SafeGo 'krunk://passwords' 2500
    TypeSecret 'polish test master'; Keys '{ENTER}' 4000
  }
  SafeGo 'krunk://about' 2500
  Shot "$P-1-about-links"
  SafeGo 'krunk://settings#permissions' 2500
  Shot "$P-2-permissions"
  SafeGo 'krunk://settings#passwords' 2500
  Shot "$P-3-passwords-links"
  SafeGo 'http://localhost:8094/login.html' 3000
  Click (X 150) (Y 228) 1500
  Shot "$P-4-login-suggestion"
  Keys '{ESC}' 300
  SafeKeys '+{ESC}' 2500
  Click (X 300) (Y 230) 600
  Click (X 300) (Y 262) 600
  MoveTo (X 900) (Y 600)
  Start-Sleep -Milliseconds 400
  Shot "$P-5-tasks-selected"
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[polish] done"
