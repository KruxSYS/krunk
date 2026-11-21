# Krunk 0.26.0 regression: the ad blocker test page (www\8092\ads.html) with --log-blocked on a
# fresh copy of lp-base (filter lists fetched). Logs "[ads TAG] blocked N" and shots ads-TAG.
param([string]$App = 'app260rel', [string]$Tag = 'new')
. C:\pipeline\krunk\lib.ps1
$K = 'C:\pipeline\krunk'; $P = "lp-ads-$Tag"
if (-not (Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like '*serve.ps1*8092*' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File',"$K\serve.ps1",'-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
Remove-Item -Recurse -Force "$K\$P" -ErrorAction SilentlyContinue; Copy-Item -Recurse "$K\lp-base" "$K\$P"
$env:LOCALAPPDATA = "$K\$P"; Remove-Item Env:KRUNK_PERF_OFF -ErrorAction SilentlyContinue
Start-Process -FilePath "$K\$App\Krunk.exe" -WorkingDirectory "$K\$App" -ArgumentList '--log-blocked','http://localhost:8092/ads.html'
Start-Sleep 15
Shot "ads-$Tag"
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
for ($i = 0; $i -lt 60 -and (Get-Process Krunk -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
$lines = @(Select-String -Path "$K\$P\Krunk\krunk.log" -Pattern ' (blocked|redirected) .*\(page localhost\)' -ErrorAction SilentlyContinue)
$lines | ForEach-Object { $_.Line } | Set-Content "$Shots\ads-$Tag.log"
Log "[ads $Tag] blocked $($lines.Count)"
