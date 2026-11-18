# Krunk 0.25.0 final verification: the main flows on the INSTALLED release (live update result),
# fresh profile, a screenshot per step. Part 1 dark, part 2 light (theme via setkv).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'final'
$Exe = "$env:USERPROFILE\AppData\Local\Programs\Krunk\Krunk.exe"
Remove-Item "$Shots\*" -Recurse -Force -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Log "[$P] installed exe $((Get-Item $Exe).VersionInfo.ProductVersion)"
$profile = 'C:\pipeline\krunk\p0250final'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
Copy-Item -Recurse C:\pipeline\krunk\sp-tpl\* $profile -ErrorAction SilentlyContinue
$env:LOCALAPPDATA = $profile
function StartKrunk([string]$theme) {
  Log "[$P] settings: $(python C:\pipeline\krunk\setkv.py $profile ('{\"theme\": \"' + $theme + '\", \"checkUpdatesAutomatically\": false}'))"
  Start-Process -FilePath $Exe
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $script:A | Out-Null
  SafeKeys '{ESC}' 300
}
function Title { [TW]::Txt($script:A) }
StartKrunk 'dark'
Shot "$P-01-start-dark"; Log "[$P] start: '$(Title)'"
SafeGo 'http://localhost:8094/dark.html' 2500
Shot "$P-02-page"; Log "[$P] page: '$(Title)'"
SafeKeys '^t' 900
Shot "$P-03-ctrl-t-ntp"; Log "[$P] Ctrl+T: '$(Title)'"
SafeGo 'http://localhost:8094/media.html' 2500
SafeKeys '^t' 900
SafeGo 'http://localhost:8094/light.html' 2500
Shot "$P-04-three-tabs"; Log "[$P] third tab: '$(Title)'"
SafeKeys '^{TAB}' 700
Shot "$P-05-ctrl-tab"; Log "[$P] Ctrl+Tab -> '$(Title)'"
SafeKeys '^{TAB}' 700
Log "[$P] Ctrl+Tab -> '$(Title)'"
SafeKeys '^2' 700
Shot "$P-06-ctrl-2"; Log "[$P] Ctrl+2 -> '$(Title)'"
SafeKeys '^w' 900
Shot "$P-07-closed"; Log "[$P] Ctrl+W -> '$(Title)'"
SafeKeys '^+t' 1500
Log "[$P] reopen closed -> '$(Title)'"
SafeGo 'krunk://settings' 2500
Shot "$P-08-settings"; Log "[$P] settings: '$(Title)'"
SafeGo 'krunk://history' 2000
Shot "$P-09-history"; Log "[$P] history: '$(Title)'"
SafeGo 'krunk://downloads' 2000
Shot "$P-10-downloads"; Log "[$P] downloads: '$(Title)'"
SafeKeys '^n' 2500
$W = [IntPtr]::Zero; foreach ($h in Frames) { if ($h -ne $script:A) { $W = $h } }
Shot "$P-11-ctrl-n"; Log "[$P] Ctrl+N: windows $(@(Frames).Count)"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
StartKrunk 'light'
Shot "$P-12-start-light"; Log "[$P] light start: '$(Title)'"
Start-Sleep 4
SafeKeys '^t' 900
Shot "$P-13-ctrl-t-light"; Log "[$P] light Ctrl+T: '$(Title)'"
$log = "$profile\Krunk\krunk.log"
Log "[$P] made-ahead lines: $(@(Select-String -Path $log -Pattern 'made ahead').Count); WARN/ERROR: $(@(Select-String -Path $log -Pattern ' WARN | ERROR ').Count)"
Select-String -Path $log -Pattern ' WARN | ERROR ' | Select-Object -First 5 | ForEach-Object { Log "[$P] $($_.Line)" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] done"
