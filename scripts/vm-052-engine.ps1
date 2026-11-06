# Krunk 0.5.2: the self-built engine (CEF 156.0.15 with proprietary codecs, snapshot_blob.bin
# instead of v8_context_snapshot.bin). Fresh profile: start-up without the "runtime damaged"
# dialog, the media probe page with its H.264/AAC clip, DASH and HLS streams playing, normal
# pages, krunk:// pages and DevTools; then the start-up lines and every WARN/ERROR of krunk.log.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'engine'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$app = 'C:\pipeline\krunk\app'
Log "[setup] libcef.dll $((Get-Item "$app\libcef.dll").Length) bytes; snapshot_blob.bin $(Test-Path "$app\snapshot_blob.bin"); v8_context_snapshot.bin $(Test-Path "$app\v8_context_snapshot.bin")"
$profile = 'C:\pipeline\krunk\p052e'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$t0 = Get-Date
Start-Process -FilePath "$app\Krunk.exe" -WorkingDirectory $app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
$A = MainWindow
Log "[start] main window after $([int]((Get-Date) - $t0).TotalMilliseconds) ms: $A"
$dialogs = @(Get-Process Krunk -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -match 'damaged|missing' })
Log "[start] 'runtime damaged' dialogs: $($dialogs.Count)"
Start-Sleep 5
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
$cx = [int](($c.Left + $c.Right) / 2); $cy = [int](($c.Top + $c.Bottom) / 2)
Shot "$P-0-newtab"

# (a) The probe page: the clips start on load; scroll with the keyboard to the players.
SafeGo 'http://localhost:8094/media.html' 20000
Shot "$P-1-codecs"
Click $cx ($c.Bottom - 40); Start-Sleep -Milliseconds 500
Keys '{PGDN}' 1500; Shot "$P-2-playback-a"
Keys '{PGDN}' 1500; Shot "$P-3-playback-b"
Keys '{END}' 1500; Shot "$P-4-playback-end"

# (b) Normal and internal pages, DevTools.
SafeKeys '^t' 1500; SafeGo 'https://example.com/' 5000
Shot "$P-5-example"
SafeKeys '{F12}' 5000
Shot "$P-6-devtools"
SafeKeys '{F12}' 2000
SafeGo 'krunk://gpu' 5000
Shot "$P-7-gpu"
SafeGo 'krunk://settings' 4000
Shot "$P-8-settings"
SafeGo 'krunk://history' 4000
Shot "$P-9-history"
$title = (Get-Process Krunk | Where-Object { $_.MainWindowTitle } | Select-Object -First 1).MainWindowTitle
Log "[pages] title now: $title"

KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
$log = "$profile\Krunk\krunk.log"
Copy-Item $log "$Shots\$P-krunk.log"
Get-Content $log | Select-Object -First 12 | ForEach-Object { Log "[log-head] $($_.Substring(0, [Math]::Min(400, $_.Length)))" }
$bad = @(Get-Content $log | Select-String ' WARN | ERROR ')
Log "[log] WARN/ERROR lines: $($bad.Count)"
$bad | ForEach-Object { Log "[log-bad] $($_.Line.Substring(0, [Math]::Min(600, $_.Line.Length)))" }
Get-Content 'C:\pipeline\krunk\www\access.log' | Where-Object { $_ -gt ($t0.ToString('o')) -and $_ -match '/log\?play' } | ForEach-Object {
  $u = [uri]($_ -split ' ')[2]; Log "[probe] $([uri]::UnescapeDataString($u.Query.TrimStart('?')))"
}
