# Krunk 0.19.0 on the test VM: a fresh profile for the WASM inspector's round with the AI agent
# and user scripts. The AI provider is the mock at 127.0.0.1:18111 (ui/dev/mock-provider.cjs on
# the build host behind `ssh -R`, as scripts/vm-0160-run.sh starts it); user scripts are on.
# Krunk is left running (DevTools port 9222) for the GUI steps that follow.
#   scripts/vm.sh ps1 scripts/vm-0190.ps1        (the app folder: C:\pipeline\krunk\app190)
# Needs lib.ps1, vm-windows.ps1, setkv.py and serve.ps1 in C:\pipeline\krunk (scripts/vm-*.{ps1,py}).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$Root = 'C:\pipeline\krunk'; $App = 'app190'; $prof = "$Root\pw19"
$Py = 'C:\Program Files\Python312\python.exe'
function StopKrunk {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  for ($i = 0; $i -lt 30 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 2
}
function StartKrunk {
  $env:LOCALAPPDATA = $prof; $env:KRUNK_EXT_LOG = '1'; $env:KRUNK_DEVTOOLS_PORT = '9222'
  Start-Process -FilePath "$Root\$App\Krunk.exe" -WorkingDirectory "$Root\$App"
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 10
  $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $A | Out-Null
}
Remove-Item "$Shots\*" -Force -ErrorAction SilentlyContinue
if (-not (Get-NetTCPConnection -State Listen -LocalPort 8094 -ErrorAction SilentlyContinue)) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$Root\serve.ps1", '-PrefixList', 'http://*:8094/'
}
StopKrunk
Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $prof | Out-Null
# One start makes the database; then the settings.
StartKrunk; StopKrunk
$patch = '{ "bookmarksBar": "never", "checkUpdatesAutomatically": false, "theme": "light", "startup": "newTab", "userscripts": {"enabled": true}, "ai": {"enabled": true, "providers": [{"id": "0a1b2c3d", "kind": "custom", "name": "Mock", "baseUrl": "http://127.0.0.1:18111/v1"}], "activeProvider": "0a1b2c3d", "activeModel": "mock-agent"} }'
[IO.File]::WriteAllText("$prof\patch.json", $patch, (New-Object Text.UTF8Encoding($false)))
$out = "$(& $Py "$Root\setkv.py" $prof "@$prof\patch.json" 2>&1)"
Set-Content "$Shots\setkv.txt" $out
StartKrunk
Shot 'z00-start'
