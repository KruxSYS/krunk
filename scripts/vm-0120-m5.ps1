# Krunk 0.12.0 M5 flows on the VM (real build, mock provider over `ssh -R 127.0.0.1:18111:…`),
# the ones M4's smoke run (vm-0120-smoke.ps1) did not cover. One phase per run:
#   probe     layouts of Settings → AI and krunk://scripts (coordinates for the others)
#   settings  a: Settings → AI (search, custom + OpenRouter providers, masked key, Test, models, Remove)
#   agent     b–e: sensitive gate, refusals, private tab, panel (resize, max steps, model switch)
#   scripts   f–g: userscripts import (GM_*), warnings, krunk:// pages; editor, hotkeys, Run
#   off       h: userscripts and macros off; AI off → no provider requests
$Phase = 'probe'
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "m5-$Phase"
$www = 'C:\pipeline\krunk\www'
$py = 'C:\Program Files\Python312\python.exe'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
$profile = "C:\pipeline\krunk\p0120-$Phase"
$env:LOCALAPPDATA = $profile
$klog = "$profile\Krunk\krunk.log"
$db = "$profile\Krunk\browser.db"
function StartKrunk {
  Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  [U]::MoveWindow($script:A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $script:A)) { Log "[$P] could not focus"; throw 'no focus' }
  $script:c = ClientRect $script:A
}
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep 4
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-Process Krunk -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
  Start-Sleep 2
}
# A fresh profile, closed again, then settings patched (PowerShell 5 drops quotes of native
# arguments: the patch goes through a file).
function FreshProfile([string]$patch) {
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $profile | Out-Null
  StartKrunk
  StopKrunk
  if ($patch) {
    [IO.File]::WriteAllText("$profile\patch.json", $patch)
    $set = & $py C:\pipeline\krunk\setkv.py $profile "@$profile\patch.json" 2>&1
    Log "[$P] setkv: $set"
  }
}
function Title { [TW]::Txt((MainWindow)) }
# Clicks the address bar (Ctrl+L after a native file dialog went nowhere), then types.
function Page([string]$url, [int]$wait = 2500) {
  Focus (MainWindow) | Out-Null
  Click 640 56 300
  SafeKeys '^a' 100
  SafeKeys $url 100
  SafeKeys '{ENTER}' $wait
}
function Ask([string]$text, [int]$wait) {
  Focus (MainWindow) | Out-Null
  Click ($c.Right - 190) ($c.Bottom - 26) 300
  SafeKeys $text 150
  SafeKeys '{ENTER}' $wait
}
function KLog([string]$pattern) {
  Get-Content $klog | Where-Object { $_ -match $pattern } | Select-Object -Last 40 | ForEach-Object { Log "[$P] klog $_" }
}
$mock = '{"ai": {"enabled": true, "maxSteps": 3, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"}}'

if ($Phase -eq 'probe') {
  FreshProfile $mock
  StartKrunk
  Page 'krunk://settings#ai'
  Shot "$P-01-settings-ai"
  SafeKeys '{PGDN}' 600
  Shot "$P-02-settings-ai-down"
  Page 'krunk://scripts'
  Shot "$P-03-scripts"
  Click 90 198 1500
  Shot "$P-04-new-userscript"
  Page 'krunk://scripts#macros'
  Shot "$P-05-macros"
  StopKrunk
}

if ($Phase -eq 'settings') {
  # a: AI on, no provider yet.
  FreshProfile '{"ai": {"enabled": true}}'
  StartKrunk
  Page 'krunk://settings'
  Click 968 146 400
  SafeKeys 'AI' 1000
  Shot "$P-01-search-ai"
  # Clear the search, then the AI and scripts tab.
  Click 1106 146 400
  Click 189 452 800
  Shot "$P-02-ai-tab-empty"
  # OpenRouter with a fake key (never tested: that would send it to openrouter.ai).
  Click 884 516 300
  SafeKeys 'sk-or-FAKE-M5-0120-KEY' 200
  Shot "$P-03-key-masked"
  Click 1060 516 1500
  Shot "$P-04-openrouter-added"
  # The custom provider: the kind select by keyboard from the name field.
  Click 675 519 300
  SafeKeys '+{TAB}' 200
  SafeKeys '{DOWN}{DOWN}{DOWN}' 600
  Shot "$P-05-custom-form"
  Click 675 520 200
  SafeKeys '^a' 100
  SafeKeys 'Mock' 100
  Click 939 520 200
  SafeKeys 'http://127.0.0.1:18111/v1' 200
  Click 1060 559 1500
  Shot "$P-06-mock-added"
  Click 942 504 2000
  Shot "$P-07-mock-test"
  # The agent's provider: Mock (second in the list), then its models.
  Click 1031 679 600
  SafeKeys '{DOWN}' 200
  SafeKeys '{ENTER}' 800
  Click 1061 735 2000
  SafeKeys '{PGDN}' 600
  Shot "$P-08-models"
  Click 741 220 300
  SafeKeys 'small' 600
  Shot "$P-09-models-search"
  Click 418 258 1200
  Shot "$P-10-model-picked"
  # The stored key while it exists: a DPAPI blob in ai_keys (never the text).
  $q = "import sqlite3; db = sqlite3.connect(r'$db'); print(db.execute('select provider_id, length(blob), hex(substr(blob, 1, 20)), instr(blob, cast(? as blob)) from ai_keys', ('sk-or-FAKE',)).fetchall())"
  Log "[$P] ai_keys with OpenRouter: $(& $py -c $q 2>&1)"
  # Remove OpenRouter (an in-page confirm: Enter takes its default button).
  Click 240 640 200
  SafeKeys '^{HOME}' 600
  Click 1073 455 800
  Shot "$P-11-remove-confirm"
  SafeKeys '{ENTER}' 1200
  Shot "$P-12-removed"
  Log "[$P] ai_keys after Remove: $(& $py -c $q 2>&1)"
  StopKrunk
  foreach ($f in @($klog, "$profile\Krunk\cef.log", "$profile\Krunk\browser.db")) {
    $hit = Select-String -Path $f -SimpleMatch 'sk-or-FAKE' -Quiet -ErrorAction SilentlyContinue
    Log "[$P] fake key text in $(Split-Path $f -Leaf): $hit"
  }
  KLog 'ai provider|ai key|settings'
}

if ($Phase -eq 'agent') {
  # b–e with the mock active (max steps 3).
  FreshProfile $mock
  StartKrunk
  Page 'http://127.0.0.1:8094/agent.html'
  SafeKeys '^+ ' 1500
  # b: the gate. "history" → consent card (Allow has the focus) → get_history runs.
  Ask 'show my history from today' 3000
  Shot "$P-b1-consent"
  SafeKeys '{ENTER}' 3000
  Shot "$P-b2-allowed"
  # A new conversation; "bookmarks" → Deny (the second button).
  Click 1227 121 800
  Ask 'show my bookmarks' 3000
  SafeKeys '{TAB}' 200
  SafeKeys '{ENTER}' 3000
  Shot "$P-b3-denied"
  # Without a keyword: no sensitive tools offered (checked in the mock's log).
  Click 1227 121 800
  Ask 'List me all the links on this page' 5000
  # c: refusals.
  Click 1227 121 800
  Ask 'go to the passwords page' 6000
  Shot "$P-c1-passwords-refused"
  Page 'krunk://settings'
  Click 1227 121 800
  Ask 'read this settings page' 6000
  Shot "$P-c2-settings-refused"
  Page 'http://127.0.0.1:8094/agent.html'
  Click 1227 121 800
  Ask 'read the password field' 5000
  Shot "$P-c3-eval-refused"
  Page 'http://127.0.0.1:8094/agent-empty.html'
  Click 1227 121 800
  Ask 'read the password field' 5000
  Shot "$P-c4-eval-redacted"
  # d: a private tab.
  Focus (MainWindow) | Out-Null
  SafeKeys '^+n' 2000
  Page 'http://127.0.0.1:8094/agent.html'
  Click 1227 121 800
  Ask 'Change the title of this page to PRIVATE' 5000
  Log "[$P] private tab title: $(Title)"
  Shot "$P-d1-private-title"
  # e: the splitter, then a tab switch keeps the width.
  $sx = 889
  Drag $sx 400 ($sx - 140) 400
  Start-Sleep 1
  Shot "$P-e1-resized"
  Focus (MainWindow) | Out-Null
  SafeKeys '^1' 1200
  Shot "$P-e2-other-tab"
  # The step limit (3): "loop" never finishes.
  Click 1227 121 800
  Ask 'loop' 6000
  Shot "$P-e3-max-steps"
  SafeKeys '{ESC}' 1000
  # The quick model switch in the header.
  Click 965 121 800
  Shot "$P-e4-model-list"
  SafeKeys '{DOWN}' 200
  SafeKeys '{ENTER}' 800
  Shot "$P-e5-model-switched"
  Click 1227 121 800
  Ask 'hello' 3000
  KLog 'agent|ai provider|panel'
  StopKrunk
}

if ($Phase -eq 'model') {
  # e: the quick model switch (the indicator at the default panel width).
  FreshProfile $mock
  StartKrunk
  Page 'http://127.0.0.1:8094/agent.html'
  SafeKeys '^+ ' 1500
  Click 965 121 1000
  Shot "$P-e4-model-list"
  Click 960 180 1200
  Shot "$P-e5-model-switched"
  Ask 'hello' 3000
  KLog 'agent|ai provider|settings'
  StopKrunk
}

function Import([string]$file) {
  Page 'krunk://scripts'
  Click 360 198 2500
  # The Windows file dialog (its own window, so not SafeKeys) takes a typed full path.
  $dlg = [IntPtr]::Zero
  for ($i = 0; $i -lt 20 -and $dlg -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 250; $dlg = [U]::FindWindow('#32770', $null) }
  Log "[$P] file dialog: $($dlg -ne [IntPtr]::Zero)"
  Keys $file 200
  Keys '{ENTER}' 2500
}
if ($Phase -eq 'scripts') {
  FreshProfile $mock
  StartKrunk
  # f: import a .user.js using document-start, GM_addStyle, GM_setValue/GM_getValue, GM_notification.
  Import 'C:\pipeline\krunk\userscripts\gm-test.user.js'
  Shot "$P-f1-imported"
  Page 'http://127.0.0.1:8094/agent.html' 1500
  Log "[$P] title after load 1: $(Title)"
  Shot "$P-f2-gm-first-load"
  Focus (MainWindow) | Out-Null; SafeKeys '{F5}' 2500
  Log "[$P] title after load 2: $(Title)"
  Shot "$P-f3-gm-reload"
  Page 'krunk://about'
  Log "[$P] krunk://about title: $(Title)"
  Shot "$P-f4-about-untouched"
  Import 'C:\pipeline\krunk\userscripts\warn.user.js'
  Shot "$P-f5-warn-imported"
  # g: the editor: a syntax error on Ctrl+S, then Ctrl+F.
  Page 'krunk://scripts'
  Click 239 198 1500
  Click 640 500 300
  SafeKeys '^{END}' 100
  SafeKeys '{ENTER}let x = ;' 200
  SafeKeys '^s' 1500
  Shot "$P-g1-syntax-error"
  SafeKeys '^f' 400
  SafeKeys 'match' 200
  SafeKeys '{ENTER}' 600
  Shot "$P-g2-find"
  SafeKeys '{ESC}' 300
  # A macro that holds A, made by the agent (then stopped), gets a hotkey: Ctrl+T is refused,
  # Ctrl+Alt+K taken.
  Page 'http://127.0.0.1:8094/agent.html'
  SafeKeys '^+ ' 1500
  Ask 'Create a macro that holds the A key' 8000
  Focus (MainWindow) | Out-Null
  SafeKeys '^+.' 1000
  SafeKeys '^+ ' 1000
  Page 'krunk://scripts#macros'
  Shot "$P-g3-macros-list"
  Click 998 257 1500
  Click 158 185 600
  SafeKeys '^t' 1000
  Shot "$P-g4-hotkey-ctrl-t-refused"
  # (Recording goes on after a refused chord: the next one is taken.)
  SafeKeys '^%k' 1000
  Click 1221 144 1500
  Shot "$P-g5-hotkey-saved"
  # A second macro can't take the same hotkey.
  Page 'krunk://scripts#macros'
  Click 229 198 1500
  Click 317 144 200
  SafeKeys 'Duplicate' 200
  Click 158 185 600
  SafeKeys '^%k' 1000
  Shot "$P-g6-hotkey-duplicate-refused"
  # The hotkey starts the macro (toolbar indicator), a second press stops it (keyup).
  Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
  Page 'http://127.0.0.1:8094/agent.html'
  Click 200 640 300
  SafeKeys '^%k' 3000
  Shot "$P-g7-hotkey-started"
  SafeKeys '^%k' 1500
  Shot "$P-g8-hotkey-stopped"
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-key=' } | Select-Object -Last 4 | ForEach-Object { Log "[$P] key $(($_ -split 'agent-key=')[1] -split ' ' | Select-Object -First 1)" }
  # The Run button in the list, in a second tab: it acts on the window's last web tab (the
  # test page), then Stop.
  Focus (MainWindow) | Out-Null
  SafeKeys '^t' 1200
  Page 'krunk://scripts#macros'
  Click 949 257 3000
  Shot "$P-g9a-run-from-scripts"
  Focus (MainWindow) | Out-Null
  SafeKeys '^1' 1500
  Shot "$P-g9-run-button"
  Focus (MainWindow) | Out-Null
  SafeKeys '^+.' 1500
  Shot "$P-g10-run-stopped"
  KLog 'macro'
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'gm-count' } | ForEach-Object { Log "[$P] $(($_ -split 'GET ')[1] -split ' cookie' | Select-Object -First 1)" }
  KLog 'userscript'
  StopKrunk
}

if ($Phase -eq 'hotkey') {
  # A macro "Hold A" with the hotkey Ctrl+Alt+K, seeded into the closed profile.
  FreshProfile $mock
  $q = & $py C:\pipeline\krunk\seed-macro.py $profile 2>&1
  Log "[$P] seeded: $q"
  StartKrunk
  Page 'http://127.0.0.1:8094/agent.html'
  Click 200 640 300
  SafeKeys '^%k' 3000
  Shot "$P-1-started"
  SafeKeys '^%k' 2000
  Shot "$P-2-stopped-by-hotkey"
  Click 200 640 300
  SafeKeys '^%k' 3000
  SafeKeys '^+.' 2000
  Shot "$P-3-stopped-by-shortcut"
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-key=' } | ForEach-Object { Log "[$P] key $(($_ -split 'agent-key=')[1] -split ' ' | Select-Object -First 1)" }
  KLog 'macro|input'
  StopKrunk
}

if ($Phase -eq 'off') {
  # h: userscripts and macros off in Settings: no injection, the hotkey does nothing; then AI off.
  FreshProfile $mock
  Log "[$P] seeded: $(& $py C:\pipeline\krunk\seed-macro.py $profile --userscript 2>&1)"
  StartKrunk
  Page 'http://127.0.0.1:8094/agent.html'
  Log "[$P] title with scripts on: $(Title)"
  Page 'krunk://settings#ai'
  Click 240 640 200
  SafeKeys '{PGDN}' 600
  Shot "$P-h1-settings-before"
  Click 1099 458 600
  Click 1099 577 600
  Shot "$P-h2-scripts-macros-off"
  Remove-Item "$www\access.log" -ErrorAction SilentlyContinue
  Page 'http://127.0.0.1:8094/agent.html'
  Log "[$P] title with scripts off: $(Title)"
  Click 200 640 300
  SafeKeys '^%k' 2500
  Shot "$P-h3-hotkey-does-nothing"
  Get-Content "$www\access.log" -ErrorAction SilentlyContinue | Where-Object { $_ -match 'agent-key=' } | ForEach-Object { Log "[$P] key $(($_ -split 'agent-key=')[1] -split ' ' | Select-Object -First 1)" }
  # AI off.
  Page 'krunk://settings#ai'
  Click 240 640 200
  SafeKeys '^{HOME}' 600
  Click 1099 302 800
  Shot "$P-h4-ai-off"
  KLog 'macro|userscript|ai provider'
  StopKrunk
}
if ($Phase -eq 'aioff') {
  $profile = 'C:\pipeline\krunk\p0120-off'; $env:LOCALAPPDATA = $profile; $klog = "$profile\Krunk\krunk.log"
  # h: the same profile with AI off, restarted: the button is gone, Ctrl+Shift+Space goes to
  # the page, and the mock must see no request (counted on the Linux side).
  StartKrunk
  Page 'http://127.0.0.1:8094/agent.html'
  SafeKeys '^+ ' 1200
  Shot "$P-h5-ai-off-restart"
  Page 'krunk://settings#ai' 2000
  Start-Sleep 20
  Shot "$P-h6-ai-off-settings"
  KLog 'ai provider|agent'
  StopKrunk
}
