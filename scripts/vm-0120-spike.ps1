# Krunk 0.12.0 M2 spike (real build, VM): the agent panel docked next to the page, a streamed
# provider request through the engine (mock provider over `ssh -R 127.0.0.1:18111:…`) and its
# cancel, engine key events with auto-repeat (agent.html's key log), whether an engine Ctrl+T
# reaches Krunk's shortcuts, Runtime.compileScript in a built-in page, Page.captureScreenshot.
# The panel's "/spike …" commands run the experiments (app/src/browser/agent/mod.rs, M2 only).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'spike'
$www = 'C:\pipeline\krunk\www'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\Downloads\krunk-spike.png" -ErrorAction SilentlyContinue
$profile = 'C:\pipeline\krunk\p0120s'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$klog = "$profile\Krunk\krunk.log"
function StartKrunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
}
# First start makes the profile; AI goes on while Krunk is closed.
StartKrunk
Shot "$P-00-ai-off"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process Krunk -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
Start-Sleep 2
# (PowerShell 5 drops the double quotes of a native argument: the patch goes through a file.)
[IO.File]::WriteAllText("$profile\patch.json", '{"ai": {"enabled": true}}')
$set = & 'C:\Program Files\Python312\python.exe' C:\pipeline\krunk\setkv.py $profile "@$profile\patch.json" 2>&1
Log "[$P] setkv: $set"
StartKrunk
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
$c = ClientRect $A
SafeGo 'http://127.0.0.1:8094/agent.html' 2500
Shot "$P-01-button"
# Ctrl+Shift+Space opens the panel.
SafeKeys '^+ ' 1500
Shot "$P-02-panel"
$px = $c.Right - 190
$py = $c.Bottom - 70
function Panel([string]$text, [int]$wait) {
  Focus (MainWindow) | Out-Null
  Click $px $py 300
  SafeKeys $text 100
  SafeKeys '{ENTER}' $wait
}
if ($env:SPIKE_FULL) {
# (b) streaming, then Stop (Esc in the panel) after 2.5 s.
Panel '/spike stream http://127.0.0.1:18111/v1/chat/completions' 2500
Shot "$P-03-streaming"
$t0 = Get-Date
SafeKeys '{ESC}' 1200
Log "[$P] Esc sent at $($t0.ToString('HH:mm:ss.fff'))"
Shot "$P-04-stopped"
# (c) key repeat into the page: click the key log first (the page has focus), then the spike.
Click ($c.Left + 200) ($c.Top + 600) 300
Panel '/spike keys' 2500
Shot "$P-05-keys"
# (c') does an engine Ctrl+T open a tab?
Panel '/spike ctrlt' 1500
Shot "$P-06-ctrlt"
# (e) screenshot of the web page.
Panel '/spike shot' 2500
Log "[$P] spike png: $(Test-Path "$env:USERPROFILE\Downloads\krunk-spike.png") $((Get-Item "$env:USERPROFILE\Downloads\krunk-spike.png" -ErrorAction SilentlyContinue).Length)"
}
# (d) compile check in a built-in page.
Focus (MainWindow) | Out-Null
Click ($c.Left + 300) ($c.Top + 300) 300
SafeGo 'krunk://about' 2500
Panel '/spike compile' 2000
Shot "$P-07-compile"
# (a) resize: drag the splitter 150 px to the left.
$sx = $c.Right - 380 - 3
Drag $sx ($c.Top + 400) ($sx - 150) ($c.Top + 400)
Start-Sleep 1
Shot "$P-08-resized"
# Ctrl+Shift+Space closes it again.
Focus (MainWindow) | Out-Null
SafeKeys '^+ ' 1200
Shot "$P-09-closed"
Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-key' } | ForEach-Object { Log "[$P] $_" }
Get-Content $klog | Where-Object { $_ -match 'spike|agent|panel|DevTools' } | ForEach-Object { Log "[$P] klog $_" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
