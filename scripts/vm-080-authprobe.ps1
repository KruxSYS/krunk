# Krunk 0.8.0 M6b probe: does the engine ask for HTTP authentication (get_auth_credentials)?
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'authprobe'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
Start-Sleep 2
$profile = "$K\p080a"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app" -ArgumentList 'http://localhost:8094/auth/area.html'
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
Focus $A | Out-Null
SafeGo 'http://localhost:8094/auth/area.html' 4000
Shot "$P-1"
Log "[$P] title '$([TW]::Txt($A))'"
if (Focus $A) { SafeKeys '%{F4}' 2500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'http auth|WARN|ERROR' | ForEach-Object { $_.Line }) -join ' ## ')"
Log "[$P] server: $((Get-Content "$K\www\access.log" -Tail 20 | Select-String ' auth ' | ForEach-Object { $_.Line }) -join ' ## ')"
