# Krunk 0.26.0 preconnect proof (F-block, F-nsp, private tabs): opens www\8092\preconnect.html
# with a netlog and copies it to C:\pipeline\krunk\out\netlog\nl-<Case>.json.
#   -Case on       blocked-host set on (the shipped behaviour): no socket/DNS for doubleclick
#   -Case off      KRUNK_PERF_OFF=preconnect-block (harness builds): the control, doubleclick hinted
#   -Case private  a private tab (Ctrl+Shift+N): no preconnect at all
#   -Case vpn      the profile -Base (VPN or kill switch already set up in it)
# Fresh copy of the base profile per case (lp-base: filter lists fetched, QUIC off).
# Analysis on the host: scripts/perf/netlog-hosts.py nl-<Case>.json example.org doubleclick.net example.net
param([string]$Case = 'on', [string]$App = 'app260n', [string]$Base = 'lp-base')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'; $P = "lp-nl-$Case"; $out = "$K\out\netlog"
New-Item -ItemType Directory -Force $out | Out-Null
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\$Base" "$K\$P"
$nl = "$out\nl-$Case.json"; Remove-Item -Force $nl -ErrorAction SilentlyContinue
$flags = @{ extraFlags = "--disable-quic --log-net-log=$nl" } | ConvertTo-Json -Compress
Set-Content -Path "$K\nl-flags.json" -Value $flags -Encoding ascii
python "$K\setkv.py" "$K\$P" "@$K\nl-flags.json" | Out-Null
$env:LOCALAPPDATA = "$K\$P"
$env:KRUNK_PERF_OFF = $(if ($Case -eq 'off') { 'preconnect-block' } else { '' })
$url = 'http://localhost:8092/preconnect.html'
if ($Case -eq 'private') {
  Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  Keys '^+n' 2500
  Keys '^l' 300; Keys $url 100; Keys '{ENTER}' 20000
} else {
  Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App" -ArgumentList $url
  Start-Sleep 20
}
Shot "nl-$Case"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
# The netlog is complete only after a clean exit (a forced kill leaves it cut off or empty).
for ($i = 0; $i -lt 120 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log "[netlog $Case] $((Get-Item $nl -ErrorAction SilentlyContinue).Length) bytes"
