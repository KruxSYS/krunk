# Krunk 0.14.0 VPN checks on the VM, one step per call (coordinates are read from the previous
# screenshot; the window sits at 0,0, 1280×760, 96 DPI). The testbed runs on dind and is reached
# through `ssh -R`: OpenVPN TCP at 127.0.0.1:18194, WireGuard through
# tools/vpn-testbed/udp-relay.py (client here on 127.0.0.1:51820, server on Linux).
#   -Step start            fresh profile, the relay, Krunk on krunk://settings#vpn; the network baseline
#   -Step click -X -Y      click, then a screenshot named -Name
#   -Step keys -Text       SafeKeys into Krunk
#   -Step dialog -X -Y -Text  click a native dialog's field, type (e.g. a path and {ENTER})
#   -Step go -Url          navigate the current tab
#   -Step private -Url     a private window on -Url
#   -Step sys              outside Krunk: the inside site from PowerShell, adapters and routes vs the baseline
#   -Step dnsclear | dnscache -Text NAME   Windows' DNS client cache (a leak check)
#   -Step quit             close Krunk, stop the relay, collect krunk.log
param([string]$Step = 'start', [int]$X = 0, [int]$Y = 0, [int]$AX = 0, [int]$AY = 0, [string]$Name = 'shot', [string]$Text = '', [string]$Url = '', [int]$Wait = 2500)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'vpn0140'
$K = 'C:\pipeline\krunk'
$App = "$K\app"
$profile = "$K\p0140"
$env:LOCALAPPDATA = $profile
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }
function Net { "adapters=$((Get-NetAdapter -IncludeHidden | Measure-Object).Count) routes=$((Get-NetRoute | Measure-Object).Count) ipv4routes=$(((Get-NetRoute -AddressFamily IPv4 | ForEach-Object { $_.DestinationPrefix + '>' + $_.NextHop }) | Sort-Object) -join ',' | ForEach-Object { [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($_))).Replace('-','').Substring(0,16) })" }

switch ($Step) {
  'start' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'udp-relay\.p[y]' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    Copy-Item -Recurse "$K\sp-tpl" $profile
    Set-Content -Encoding ascii "$K\patch.json" '{"checkUpdatesAutomatically":false}'
    Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
    Start-Process python -WindowStyle Hidden -ArgumentList "$K\vpn\udp-relay.py", 'client', '--udp', '127.0.0.1:51820', '--tcp', '127.0.0.1:18551'
    Set-Content "$K\vpn\net-before.txt" (Net)
    Log "[$P] network before: $(Net)"
    Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 4
    $A = MainWindow
    [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
    Focus $A | Out-Null
    Log "[$P] version $((Get-Item "$App\Krunk.exe").VersionInfo.ProductVersion)"
    SafeGo 'krunk://settings#vpn' 3000
    Shot "$P-$Name"
  }
  # Opens the VPN popup from the toolbar (-X -Y) and clicks in it (-AX -AY) without refocusing the
  # window in between (focusing the window closes popups).
  'popup' { Focus (MainWindow) | Out-Null; Click $X $Y 1500; Click $AX $AY $Wait; Shot "$P-$Name" }
  'resume' {
    Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 4
    $A = MainWindow
    [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
    Focus $A | Out-Null
    Shot "$P-$Name"
  }
  'click' { Focus (MainWindow) | Out-Null; Click $X $Y $Wait; Shot "$P-$Name" }
  'keys' { SafeKeys $Text $Wait; Shot "$P-$Name" }
  # A native dialog is in front (SafeKeys types only into a Krunk window): click its field first.
  'dialog' { Click $X $Y 400; Keys $Text $Wait; Shot "$P-$Name" }
  'go' { Focus (MainWindow) | Out-Null; SafeGo $Url $Wait; Shot "$P-$Name" }
  'private' { Focus (MainWindow) | Out-Null; SafeKeys '^+n' 2500; SafeGo $Url $Wait; Shot "$P-$Name" }
  'sys' {
    foreach ($u in 'http://inside.test/', 'http://10.77.0.10/') {
      try { $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 8 $u; Log "[$P] outside Krunk $u -> $($r.StatusCode) $($r.Content)" }
      catch { Log "[$P] outside Krunk $u -> refused: $($_.Exception.Message)" }
    }
    $before = Get-Content "$K\vpn\net-before.txt"
    $now = Net
    Log "[$P] network now: $now (same as before Krunk's VPN: $($now -eq $before))"
    Log "[$P] Krunk's processes: $((Get-Process Krunk -ErrorAction SilentlyContinue | Measure-Object).Count); vpn lines: $(KLog 'vpn')"
  }
  # Whether Windows' resolver was asked for a name (-Text): with the VPN on, Krunk resolves inside
  # the tunnel, so the name must not be in the DNS client cache.
  'dnscache' {
    $hits = @(Get-DnsClientCache -ErrorAction SilentlyContinue | Where-Object { $_.Entry -like "*$Text*" })
    Log "[$P] Windows DNS cache entries for '$Text': $($hits.Count)"
  }
  'dnsclear' { Clear-DnsClientCache; Log "[$P] Windows DNS cache cleared" }
  'quit' {
    if (Focus (MainWindow)) { SafeKeys '%{F4}' 2500 }
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'udp-relay\.p[y]' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
    Start-Sleep 1
    Log "[$P] WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
    Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
  }
}
Log "[$P] $Step $Name done"
