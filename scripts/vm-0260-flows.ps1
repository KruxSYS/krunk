# Krunk 0.26.0 final verification: the main flows on the INSTALLED copy (after the in-app update
# from 0.25.5), the user's real profile, real network. Shots flow-*; logs version, hang state,
# the blocked-host push time, PANIC lines and crash events.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
Add-Type -Namespace H -Name W -MemberDefinition '[DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h);' -ErrorAction SilentlyContinue
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'; $data = Join-Path $env:LOCALAPPDATA 'Krunk'
$t00 = Get-Date
Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
Start-Process -FilePath "$inst\Krunk.exe" -WorkingDirectory $inst
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 5
Focus $A | Out-Null
Log "[flows] exe $((Get-Item "$inst\Krunk.exe").VersionInfo.ProductVersion); hung at 5 s: $([H.W]::IsHungAppWindow($A))"
Keys '^t' 1500; Shot 'flow-01-new-tab'
Go 'https://www.theguardian.com/international' 8000; Shot 'flow-02-typed-guardian'
Go 'https://en.wikipedia.org/wiki/Web_browser' 6000; Shot 'flow-03-typed-wikipedia'
Click 230 470 6000; Shot 'flow-04-link-click'
Keys '%{LEFT}' 3000; Shot 'flow-05-back'
Keys '%{RIGHT}' 3000; Shot 'flow-06-forward'
Keys '^l' 400; Keys 'wik' 1200; Shot 'flow-07-omnibox-suggestions'; Keys '{ESC}' 200; Keys '{ESC}' 300
Keys '^+n' 2500; Go 'https://www.bbc.com/news' 7000; Shot 'flow-08-private-window'; Keys '^w' 1500
Focus (MainWindow) | Out-Null
Go 'http://localhost:8092/ads.html' 5000; Shot 'flow-09-ad-blocker'
Go 'krunk://settings' 3000; Click 960 146 400; Keys 'preload' 2000; Shot 'flow-10-settings-preload'
Log "[flows] hung at the end: $([H.W]::IsHungAppWindow((MainWindow)))"
$push = (Select-String -Path "$data\krunk.log" -Pattern 'blocked host\(s\) handed' | Select-Object -Last 2 | % Line) -join ' | '
$panic = @(Select-String -Path "$data\krunk.log" -Pattern 'PANIC').Count
$crash = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000; StartTime = $t00 } -ErrorAction SilentlyContinue | Where-Object { $_.Message -like '*Krunk.exe*' }).Count
Log "[flows] push: $push; PANIC lines: $panic; crash events: $crash"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }; Start-Sleep 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[flows] done"
