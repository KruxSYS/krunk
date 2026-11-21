# Krunk 0.26.0 smoke and VPN checks (scripts/perf/smoke-0260.sh runs the steps in order).
#   -Step live      release build (-App app260rel), fresh copy of lp-base: corpus page (live), 3 live
#                   sites, back/forward, private tab, ad test page; shots s0260-*
#   -Step vpnstart  harness build of the release (-App app260r) with the DevTools port 9222 and a
#                   netlog (-Name vpn|ks: out\netlog\nl-NAME.json), fresh copy of lp-base; d0254.mjs
#                   drives it over CDP
#   -Step dialog    type -Path into the open file dialog (the VPN profile import)
#   -Step shot      screenshot s0260-NAME
#   -Step quit      clean exit (the netlog is complete only then)
param([string]$Step = 'live', [string]$App = 'app260rel', [string]$Name = 'shot', [string]$Path = '')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'
function Fresh([string]$p) {
  Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
  Remove-Item -Recurse -Force "$K\$p" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$p"
  $env:LOCALAPPDATA = "$K\$p"; Remove-Item Env:KRUNK_PERF_OFF -ErrorAction SilentlyContinue
}
function Quit {
  Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  for ($i = 0; $i -lt 120 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
}
switch ($Step) {
  'live' {
    Fresh 'lp-smoke'
    Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
    for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 6
    [void][U]::SetForegroundWindow($A); Start-Sleep -Milliseconds 800
    Go 'https://www.theguardian.com/international' 9000; Shot 's0260-corpus-guardian'
    Go 'https://en.wikipedia.org/wiki/Rust_{(}programming_language{)}' 7000; Shot 's0260-live-wikipedia'
    Go 'https://www.bbc.com/news' 8000; Shot 's0260-live-bbc'
    Go 'https://www.youtube.com/' 9000; Shot 's0260-live-youtube'
    Click 640 450 300; Keys '%{LEFT}' 3000; Shot 's0260-back'
    Keys '%{RIGHT}' 3000; Shot 's0260-forward'
    Keys '^+n' 2500; Go 'https://en.wikipedia.org/wiki/Web_browser' 6000; Shot 's0260-private'
    Keys '^w' 1500
    Go 'http://localhost:8092/ads.html' 5000; Shot 's0260-adblock'
    Quit; Log 'smoke live done'
  }
  'vpnstart' {
    Fresh "lp-vpn-$Name"
    $nl = "$K\out\netlog\nl-$Name.json"; New-Item -ItemType Directory -Force "$K\out\netlog" | Out-Null
    Remove-Item -Force $nl -ErrorAction SilentlyContinue
    Set-Content -Path "$K\nl-flags.json" -Value (@{ extraFlags = "--disable-quic --log-net-log=$nl" } | ConvertTo-Json -Compress) -Encoding ascii
    python "$K\setkv.py" "$K\lp-vpn-$Name" "@$K\nl-flags.json" | Out-Null
    $env:KRUNK_DEVTOOLS_PORT = '9222'
    # Harness builds import -Path at start (KRUNK_VPN_IMPORT), no file dialog.
    if ($Path) { $env:KRUNK_VPN_IMPORT = $Path } else { Remove-Item Env:KRUNK_VPN_IMPORT -ErrorAction SilentlyContinue }
    Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
    for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 8
    Log "vpnstart $Name"
  }
  'dialog' { Keys "$Path{ENTER}" 3000; Log "typed $Path" }
  'shot' { Shot "s0260-$Name" }
  'quit' { Quit; Log "quit ($Name)" }
}
