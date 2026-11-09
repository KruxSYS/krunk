# Krunk 0.11.3 VM flow: a PDF sent as a download (serve.ps1 sends .pdf as octet-stream) opens
# in the viewer in a new tab; a PDF typed into the address bar too; an email link with the
# default (the system's app) and with Gmail. Screenshots "pm-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'pm'
$K = 'C:\pipeline\krunk'
if (-not (Get-NetTCPConnection -LocalPort 8094 -State Listen -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://*:8094/'
  Start-Sleep 2
}
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4
  $script:A = MainWindow
  [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Focus $A | Out-Null
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
$profile = "$K\p0113"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$before = @(Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'test*.pdf' -ErrorAction SilentlyContinue).Count
StartKrunk
SafeGo 'http://localhost:8094/mail.html' 2500
Click 40 145 3500                                   # "a PDF" (second line of mail.html)
Shot "$P-1-pdf-link"
Log "[$P] after the PDF link: title '$([TW]::Txt($A))'"
SafeKeys '^1' 600
SafeGo 'http://localhost:8094/test.pdf' 3500
Shot "$P-2-pdf-typed"
Log "[$P] after typing the PDF: title '$([TW]::Txt($A))'"
SafeKeys '^1' 600
Click 60 120 2500                                   # mailto, system default
Shot "$P-3-mailto-system"
Log "[$P] OpenWith/mail processes: $((Get-Process OpenWith, HxOutlook, olk, outlook, thunderbird -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ',')"
Get-Process OpenWith -ErrorAction SilentlyContinue | Stop-Process -Force
foreach ($f in Frames) { if (Focus $f) { Keys '%{F4}' 1200 } }
Start-Sleep 2; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[$P] setkv: $(python "$K\setkv.py" $profile '{\"mailHandler\": \"gmail\"}')"
StartKrunk
SafeGo 'http://localhost:8094/mail.html' 2500
Click 60 120 3500                                   # mailto, Gmail
Shot "$P-4-mailto-gmail"
Log "[$P] after the Gmail email link: title '$([TW]::Txt($A))'"
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
$after = @(Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'test*.pdf' -ErrorAction SilentlyContinue).Count
Log "[$P] test*.pdf in Downloads: before $before, after $after (the viewer must not save)"
Get-ChildItem "$profile\Krunk\pdf-view" -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { Log "[$P] viewer file $($_.FullName)" }
Get-Content "$profile\Krunk\krunk.log" | Select-String 'pdf|email|mailto|download|WARN|ERROR|blocked' | ForEach-Object { Log "[log] $_" }
Log "[$P] done"
