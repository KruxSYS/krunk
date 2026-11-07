# Krunk 0.7.0 M2/M3 (R7, R24): what a background tab looks like to the engine. vis.html reports
# document.visibilityState and its animation frames per second to /log while it is the active
# tab and while another tab is in front.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'vis'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p070v"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$t0 = (Get-Date).ToString('o')
SafeGo 'http://localhost:8094/vis.html?tag=krunk-tab' 5000
SafeKeys '^t' 6000
SafeKeys '^1' 3000
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
foreach ($line in Get-Content "$K\www\access.log" | Where-Object { $_ -gt $t0 -and $_ -match '/log\?krunk-tab' }) {
  Log "[$P] $($line.Substring(33, [Math]::Min(120, $line.Length - 33)))"
}
