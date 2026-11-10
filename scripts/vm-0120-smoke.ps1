# Krunk 0.12.0 smoke test on the VM (real build) with the mock provider
# (ui/dev/mock-provider.cjs on Linux, reached through `ssh -R 127.0.0.1:18111:127.0.0.1:18111`).
# Flows (plan §8, acceptance 3):
#   1 AI off by default: no agent button; on (with the mock as a custom provider): the button
#     next to the shield, the panel opens
#   2 "Change the title of this page to EXAMPLE"      → the window title
#   3 "List me all the links on this page"            → the answer lists them
#   4 "Save a screenshot of this page"                 → a PNG in Downloads
#   5 "Create me a script that will remove the banner" → banner gone, script on krunk://scripts
#   6 "Create a macro that holds the A key"           → keydown repeats, Stop → keyup
#   7 "Fill in the form"                               → name/email filled, password untouched
#   8 "ask" → the ask_user card (its answer is clicked by $AskX/$AskY when given)
#   9 "slow" → Stop (Esc) → "Stopped"; krunk.log "stopped in N ms"
param([int]$AskX = 0, [int]$AskY = 0, [string]$Only = '')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'smoke'
$www = 'C:\pipeline\krunk\www'
$dl = "$env:USERPROFILE\Downloads"
function Want([string]$flow) { -not $Only -or ($Only.Split(',') -contains $flow) }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
Get-ChildItem $dl -Filter 'Screenshot*.png' -ErrorAction SilentlyContinue | Remove-Item -Force
$profile = 'C:\pipeline\krunk\p0120'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
$klog = "$profile\Krunk\krunk.log"
function StartKrunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $script:A)) { Log "[$P] could not focus"; throw 'no focus' }
  $script:c = ClientRect $script:A
}
function Title { [TW]::Txt((MainWindow)) }
function Page([string]$url) { Focus (MainWindow) | Out-Null; SafeGo $url 2500 }
# The panel's message box (bottom of the panel, right side of the window).
function Ask([string]$text, [int]$wait) {
  Focus (MainWindow) | Out-Null
  Click ($c.Right - 190) ($c.Bottom - 26) 300
  SafeKeys $text 150
  SafeKeys '{ENTER}' $wait
}
function NewConversation {
  # Ctrl+Shift+Space twice keeps the panel; "New" is in its header (right side).
  Focus (MainWindow) | Out-Null
}

# ---- 1: AI off by default ----
StartKrunk
Page 'http://127.0.0.1:8094/agent.html'
Shot "$P-01a-ai-off"
SafeKeys '^+ ' 800
Shot "$P-01b-ai-off-ctrl-shift-space"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process Krunk -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
Start-Sleep 2
# AI on, the mock as a custom provider (no key), its tool model active.
$patch = '{"ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"}}'
[IO.File]::WriteAllText("$profile\patch.json", $patch)
$set = & 'C:\Program Files\Python312\python.exe' C:\pipeline\krunk\setkv.py $profile "@$profile\patch.json" 2>&1
Log "[$P] setkv: $set"
StartKrunk
Page 'http://127.0.0.1:8094/agent.html'
Shot "$P-01c-ai-on-button"
SafeKeys '^+ ' 1500
Shot "$P-01d-panel"

if (Want 'title') {
  Ask 'Change the title of this page to EXAMPLE' 6000
  Log "[$P] title after: $(Title)"
  Shot "$P-02-title"
}
if (Want 'links') {
  Ask 'List me all the links on this page' 6000
  Shot "$P-03-links"
}
if (Want 'shot') {
  Ask 'Save a screenshot of this page' 6000
  $png = Get-ChildItem $dl -Filter 'Screenshot*.png' -ErrorAction SilentlyContinue | Select-Object -First 1
  Log "[$P] screenshot file: $($png.Name) $($png.Length)"
  Shot "$P-04-screenshot"
  # The saved file on the downloads list.
  Page 'krunk://downloads'
  Shot "$P-04b-downloads-list"
  Page 'http://127.0.0.1:8094/agent.html'
}
if (Want 'banner') {
  Ask 'Create me a script that will remove the banner' 8000
  Shot "$P-05a-banner-removed"
  # A reload: the userscript runs at document-start.
  Focus (MainWindow) | Out-Null; Click ($c.Left + 300) ($c.Top + 400) 200; SafeKeys '{F5}' 3000
  Shot "$P-05b-banner-after-reload"
}
if (Want 'macro') {
  # The key log gets the focus first (the macro types into the page).
  Focus (MainWindow) | Out-Null; Click ($c.Left + 200) ($c.Top + 640) 300
  Ask 'Create a macro that holds the A key' 9000
  Shot "$P-06a-macro-running"
  Start-Sleep 3
  Shot "$P-06a2-macro-still-running"
  Focus (MainWindow) | Out-Null
  SafeKeys '^+.' 1500
  Shot "$P-06b-macro-stopped"
  Start-Sleep 2
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-key=' } | Select-Object -Last 8 | ForEach-Object { Log "[$P] key $(($_ -split 'agent-key=')[1] -split ' ' | Select-Object -First 1)" }
}
if (Want 'form') {
  Page 'http://127.0.0.1:8094/agent.html'
  Ask 'Fill in the form' 9000
  Shot "$P-07-form"
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-form=' } | Select-Object -Last 2 | ForEach-Object { Log "[$P] form $(($_ -split 'agent-form=')[1] -split ' ' | Select-Object -First 1)" }
}
if (Want 'ask') {
  Ask 'ask me something' 4000
  Shot "$P-08a-ask"
  # The first option has the focus: Tab to the second ("Green") and choose it.
  SafeKeys '{TAB}' 300
  SafeKeys '{ENTER}' 4000
  Shot "$P-08b-answered"
}
if (Want 'stop') {
  Ask 'slow' 2500
  Shot "$P-09a-streaming"
  SafeKeys '{ESC}' 1000
  Shot "$P-09b-stopped"
}
if (Want 'scripts') {
  Page 'krunk://scripts'
  Shot "$P-10a-scripts"
  Page 'krunk://scripts#macros'
  Shot "$P-10b-macros"
}
Get-Content $klog | Where-Object { $_ -match 'agent|userscript|macro|panel|provider' } | Select-Object -Last 80 | ForEach-Object { Log "[$P] klog $_" }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
