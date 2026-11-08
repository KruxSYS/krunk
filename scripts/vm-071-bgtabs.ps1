$Mode = ''
# Krunk 0.7.1 M3 (R24): is a background tab hidden from the engine? $Mode '' measures the release;
# 'resize' and 'widget' passed --hide-tabs=<mode> to trial builds (0x0 size; the engine's widget
# window hidden). Neither hid anything; the switches are not in any release
# (.pipeline/evidence-0.7.1/m3/README.txt). vis.html reports its visibility, animation frames
# per second, size and resize events to /log; audio.html (?report) its playing position.
#   1  vis.html in front 5 s, behind a new tab 10 s, in front again 5 s (shot: not blank)
#   2  the same with 40 s behind
#   3  audio.html playing, behind a new tab 10 s (position goes on?), in front again
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "bg-$(if ($Mode) { $Mode } else { 'default' })"
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = "$K\p071bg"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$argv = @(); if ($Mode) { $argv = @("--hide-tabs=$Mode") }
if ($argv.Count) { Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app" -ArgumentList $argv } else { Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app" }
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 5
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$t0 = (Get-Date).ToString('o')
function Mark([string]$what) { Invoke-WebRequest -UseBasicParsing "http://localhost:8094/log?$([uri]::EscapeDataString("MARK $what"))" | Out-Null }
# (1) and (2)
SafeGo 'http://localhost:8094/vis.html?tag=bgtab' 5000
Mark 'vis in front'
Start-Sleep 5
Mark 'vis behind a new tab (10 s)'
SafeKeys '^t' 10000
Mark 'vis in front again'
SafeKeys '^1' 2500
Shot "$P-1-back-after-10s"
Start-Sleep 3
Mark 'vis behind a new tab (40 s)'
SafeKeys '^2' 40000
Mark 'vis in front again after 40 s'
SafeKeys '^1' 2500
Shot "$P-2-back-after-40s"
Start-Sleep 3
# (3) audio
SafeKeys '^2' 1500
SafeGo 'http://localhost:8094/audio.html?report' 3000
$c = ClientRect (MainWindow)
Click ([int]($c.Left + 300 * $c.Scale)) ([int]($c.Top + 400 * $c.Scale)) 300
Keys 'p' 3000
Mark 'audio playing, behind a new tab (10 s)'
SafeKeys '^t' 10000
Mark 'audio tab in front again'
SafeKeys '^2' 3000
Shot "$P-3-audio-back"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log" -ErrorAction SilentlyContinue
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
foreach ($line in Get-Content "$K\www\access.log" | Where-Object { $_ -gt $t0 -and $_ -match '/log\?(bgtab|audio-tab|MARK)' }) {
  $q = $line -replace '^.*?/log\?', '' -replace ' cookie=.*$', ''
  Log "[$P] $($line.Substring(11, 8)) $([uri]::UnescapeDataString($q))"
}
Log "[$P] done"
