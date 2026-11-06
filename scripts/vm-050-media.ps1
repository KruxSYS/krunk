# Krunk 0.5.0 milestone 1: media, Local Network Access and GPU diagnosis (the Plex s4032 item).
# vm-www/8094/media.html probes the codec matrix, plays local MP4 (H.264/AAC) and WebM (VP9/Opus)
# clips plus a public DASH and HLS stream, and reads the WebGL/WebGPU renderer; it reports every
# result through the test server's access log. Then, from https://example.com, DevTools console
# snippets fetch http://localhost:8094 (loopback: Local Network Access permission) and load an
# http LAN video (mixed content); the outcome is put in the page title. Finally chrome://gpu (a
# temporary dev-build allowance) and the --probe-gpu log line (SystemInfo.getInfo). Fresh profile.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'media'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$lan = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notmatch 'Loopback' -and $_.IPAddress -notlike '169.*' } | Select-Object -First 1).IPAddress
Log "[setup] LAN address $lan"
$mark = (Get-Date -Format o)
$profile = 'C:\pipeline\krunk\p050m'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -ArgumentList '--probe-gpu' -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A

# (a) The probe page: codec matrix, local clips, DASH, HLS, WebGL.
SafeGo 'http://localhost:8094/media.html' 20000
Shot "$P-1-codecs"
Wheel ([int](($c.Left + $c.Right) / 2)) ([int](($c.Top + $c.Bottom) / 2)) -8; Start-Sleep 1
Shot "$P-2-playback"
Wheel ([int](($c.Left + $c.Right) / 2)) ([int](($c.Top + $c.Bottom) / 2)) -8; Start-Sleep 1
Shot "$P-3-gpu"
$gpuProcs = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'Krunk.exe' -and $_.CommandLine -like '*--type=gpu-process*' })
Log "[gpu] GPU processes: $($gpuProcs.Count)"
foreach ($g in $gpuProcs) { Log "[gpu] cmdline: $($g.CommandLine)" }

# (b) Local Network Access and mixed content from a public https page, through the DevTools console.
SafeKeys '^t' 1500; SafeGo 'https://example.com/' 4000
SafeKeys '{F12}' 4000
Keys '{ESC}' 1500    # the console drawer, focused
function Snippet([string]$label, [string]$js) {
  Keys $js 300; Keys '{ENTER}' 3500
  $title = (Get-Process Krunk | Where-Object { $_.MainWindowTitle } | Select-Object -First 1).MainWindowTitle
  Log "[$label] title: $title"
  Shot "$P-$label"
}
# SendKeys escapes: ( ) { } + ^ % ~ [ ] are typed as {(} {)} {{} {}} {+} ...
Snippet '4-lna-loopback' "fetch{(}'http://localhost:8094/log?lna=loopback-from-example'{)}.then{(}r=>document.title='LNA-LOOPBACK-OK '{+}r.status,e=>document.title='LNA-LOOPBACK-FAIL '{+}e.message{)}"
Snippet '5-lna-lan' "fetch{(}'http://$($lan):8094/log?lna=lan-from-example'{)}.then{(}r=>document.title='LNA-LAN-OK '{+}r.status,e=>document.title='LNA-LAN-FAIL '{+}e.message{)}"
Snippet '6-mixed-video' "v=document.createElement{(}'video'{)};v.muted=true;v.src='http://$($lan):8094/test-h264-aac.mp4';v.onerror={(}{)}=>document.title='MIXED-VIDEO-FAIL '{+}v.error.code;v.onloadedmetadata={(}{)}=>document.title='MIXED-VIDEO-OK';document.body.appendChild{(}v{)}"
Snippet '7-mixed-video-localhost' "w=document.createElement{(}'video'{)};w.muted=true;w.src='http://localhost:8094/test-h264-aac.mp4';w.onerror={(}{)}=>document.title='LOCALHOST-VIDEO-FAIL '{+}w.error.code;w.onloadedmetadata={(}{)}=>document.title='LOCALHOST-VIDEO-OK';document.body.appendChild{(}w{)}"
Keys '{F12}' 1500

# (c) chrome://gpu (temporary allowance in this dev build) and the SystemInfo probe.
SafeKeys '^t' 1500; SafeGo 'chrome://gpu' 6000
Shot "$P-8-chrome-gpu"
Wheel ([int](($c.Left + $c.Right) / 2)) ([int](($c.Top + $c.Bottom) / 2)) -10; Start-Sleep 1
Shot "$P-9-chrome-gpu-2"

KrunkWindows $P
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
Get-Content "$profile\Krunk\krunk.log" | Select-String 'browser arguments|PROBE|WARN|ERROR|blocked navigation' | ForEach-Object { Log "[log] $($_.Line.Substring(0, [Math]::Min(3000, $_.Line.Length)))" }
Get-Content 'C:\pipeline\krunk\www\access.log' | Where-Object { $_ -gt $mark -and $_ -match '/log\?' } | ForEach-Object {
  $u = [uri]($_ -split ' ')[2]; Log "[probe] $([uri]::UnescapeDataString($u.Query.TrimStart('?')))"
}
