# Krunk 0.3.0: a tab with DevTools open is never put to sleep. Fresh profile, sleep timers in
# seconds (--sleep-test-seconds=10): tab 1 has DevTools, tab 2 not, tab 3 is active; after 45 s
# tab 2 must be asleep ("low memory") and tab 1 not. Needs serve.ps1 on localhost:8092.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'dt-sleep'
$profile = 'C:\pipeline\krunk\p030m'
$env:LOCALAPPDATA = $profile
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
}
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -ArgumentList '--sleep-test-seconds=10' -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { throw 'no focus' }
SafeGo 'http://localhost:8092/move.html?sleep=1' 3000
SafeKeys '{F12}' 3000
SafeKeys '^t' 1200; SafeGo 'https://example.com/' 3000
SafeKeys '^t' 1200; SafeGo 'https://example.org/' 3000
Start-Sleep 45
Shot "$P-after-45s"
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Get-Content "$profile\Krunk\krunk.log" | Select-String 'tab \d|DevTools|WARN|ERROR' | ForEach-Object { Log "[$P] log: $_" }
