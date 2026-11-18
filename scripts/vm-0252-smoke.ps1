# Krunk 0.25.2 smoke on the test VM (one pass, the release build in C:\pipeline\krunk\app):
#  1. the wheel scrolls the page while a tooltip (title / alt text) is showing;
#  2. the login suggestions close on a click outside on a page that cancels mousedown;
#  3. the agent panel takes a pasted picture, PDF and text file (mock provider at 127.0.0.1:18111
#     behind ssh -R; the build host checks its request log).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$Root = 'C:\pipeline\krunk'; $prof = "$Root\p0252"; $P = 's0252'
$Py = 'C:\Program Files\Python312\python.exe'
Remove-Item "$Shots\*" -Recurse -Force -ErrorAction SilentlyContinue
if (-not (Get-NetTCPConnection -State Listen -LocalPort 8094 -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$Root\serve.ps1", '-PrefixList', 'http://*:8094/'
  Start-Sleep 2
}
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 2
}
function StartKrunk {
  $env:LOCALAPPDATA = $prof
  Start-Process -FilePath "$Root\app\Krunk.exe" -WorkingDirectory "$Root\app"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 8
  $script:A = MainWindow; [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $script:A | Out-Null
}
function Title { [TW]::Txt($script:A) }
function PopupShown {
  $pids = @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  @([TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' }).Count
}
StopKrunk
Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $prof | Out-Null
StartKrunk; StopKrunk
$patch = '{ "bookmarksBar": "never", "checkUpdatesAutomatically": false, "theme": "light", "startup": "newTab", "ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1", "vision": true}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"} }'
[IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
Log "[$P] setkv: $(& $Py "$Root\setkv.py" $prof "@$prof\patch.json" 2>&1)"
StartKrunk
Log "[$P] version $((Get-Item "$Root\app\Krunk.exe").VersionInfo.ProductVersion)"
SafeGo 'http://localhost:8094/s0252.html' 3000
# Calibrate: a press at a screen point gives its client point in the title.
Click 900 600 400
$t = Title; Log "[$P] calibrate: '$t'"
$m = [regex]::Match($t, 'press (\d+),(\d+)')
$ox = 900 - [int]$m.Groups[1].Value; $oy = 600 - [int]$m.Groups[2].Value
Log "[$P] page origin on screen: $ox,$oy"

# 1. Tooltip + wheel.
MoveTo ($ox + 300) ($oy + 420); Start-Sleep -Milliseconds 300
MoveTo ($ox + 310) ($oy + 430); Start-Sleep 2
Shot "$P-1a-tooltip"
KrunkWindows "$P tooltip"
Wheel ($ox + 310) ($oy + 430) 3; Start-Sleep 1
$t = Title; Log "[$P] RESULT tooltip-scroll: '$t' => $(if ($t -match '^Y=([1-9]\d*)') { 'PASS' } else { 'FAIL' })"
Shot "$P-1b-scrolled"
SafeKeys '{HOME}' 800
SafeKeys '^r' 3000

# 2. Suggestions close on a press outside.
Click ($ox + 190) ($oy + 58) 1500
$before = PopupShown
Shot "$P-2a-suggest"
Log "[$P] after click in the field: '$(Title)' popups=$before"
Click ($ox + 900) ($oy + 500) 1200
$after = PopupShown
Shot "$P-2b-outside"
Log "[$P] after click outside: '$(Title)' popups=$after"
Log "[$P] RESULT suggest-close: shown=$before after=$after => $(if ($before -ge 1 -and $after -eq 0) { 'PASS' } elseif ($before -eq 0) { 'NO-POPUP' } else { 'FAIL' })"

# 3. Agent panel: paste a picture, then a PDF and a text file (file drop list), send.
SafeKeys '^+ ' 2500
Shot "$P-3a-panel"
$bmp = New-Object Drawing.Bitmap 400, 240
$g = [Drawing.Graphics]::FromImage($bmp); $g.Clear([Drawing.Color]::SteelBlue)
$g.DrawString('KRUNK 0252', (New-Object Drawing.Font 'Arial', 36), [Drawing.Brushes]::White, 20, 80); $g.Dispose()
[Windows.Forms.Clipboard]::SetImage($bmp)
Keys '^v' 1500
'%PDF-1.4' + "`n1 0 obj<<>>endobj`ntrailer<<>>`n%%EOF" | Set-Content -NoNewline -Encoding ascii "$Root\s0252.pdf"
"name,value`nkrunk,252`n" | Set-Content -NoNewline -Encoding utf8 "$Root\s0252.csv"
$files = New-Object Collections.Specialized.StringCollection
[void]$files.Add("$Root\s0252.pdf"); [void]$files.Add("$Root\s0252.csv")
[Windows.Forms.Clipboard]::SetFileDropList($files)
Keys '^v' 1500
Shot "$P-3b-chips"
Keys 'smoke 0252 attachments' 200
Keys '{ENTER}' 4000
Shot "$P-3c-sent"
$log = "$prof\Krunk\krunk.log"
Select-String -Path $log -Pattern 'attached|login suggestions|WARN|ERROR' | Select-Object -Last 12 | ForEach-Object { Log "[$P] log: $($_.Line)" }
StopKrunk
Log "[$P] done"
