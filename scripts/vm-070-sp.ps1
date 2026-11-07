$Browser = 'krunk'; $Prof = 'sp-A'; $Tag = 'A1'; $Bg = 0; $App = 'C:\pipeline\krunk\app'; $Off = ''
# Krunk 0.7.0 M0/M3 (R24): starts one Speedometer 3.1 run (served by speedometer\sp-serve.py on
# 127.0.0.1:8096) in Krunk, Chrome or Edge with the given profile, optionally behind $Bg busy
# background tabs, and returns; scripts/speedometer/matrix.sh waits for results\latest.json.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "sp-$Tag"
Get-Process Krunk, chrome, msedge -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.CommandLine -match 'sp-[s]erve\.py' })) {
  Start-Process python -WindowStyle Hidden -ArgumentList 'C:\pipeline\krunk\speedometer\sp-serve.py', '8096'
  Start-Sleep 2
}
Remove-Item "C:\pipeline\krunk\speedometer\results\latest.json", "C:\pipeline\krunk\speedometer\results\errors.txt" -ErrorAction SilentlyContinue
# Every run starts from the same profile: Krunk's from <profile>.base (scripts/vm-070-sp-prep.ps1),
# Chrome's and Edge's from nothing.
Remove-Item -Recurse -Force "C:\pipeline\krunk\$Prof" -ErrorAction SilentlyContinue
if ($Browser -eq 'krunk') { Copy-Item -Recurse "C:\pipeline\krunk\$Prof.base" "C:\pipeline\krunk\$Prof" }
$url = "http://127.0.0.1:8096/index.html?startAutomatically=true"
$sites = @('https://www.cnn.com/', 'https://www.theverge.com/', 'https://www.youtube.com/', 'https://www.reddit.com/', 'https://www.bbc.com/news')
if ($Browser -eq 'chrome' -or $Browser -eq 'edge') {
  $argv = @("--user-data-dir=C:\pipeline\krunk\$Prof", '--no-first-run', '--no-default-browser-check', '--disable-search-engine-choice-screen', '--start-maximized', $url) + $sites[0..($Bg - 1)]
  if ($Bg -eq 0) { $argv = $argv[0..5] }
  $exe = if ($Browser -eq 'edge') { 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe' } else { 'C:\Program Files\Google\Chrome\Application\chrome.exe' }
  Start-Process $exe -ArgumentList $argv
  Start-Sleep 8
  Log "[$P] $Browser started: $($argv -join ' ')"
} else {
  $env:LOCALAPPDATA = "C:\pipeline\krunk\$Prof"
  $env:KRUNK_PERF_OFF = $Off   # perf-probe builds only (scripts/build-dist.sh --probe)
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 6
  $A = MainWindow
  [U]::ShowWindow($A, 3) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log "[$P] could not focus window $A"; Shot "$P-nofocus"; throw "no focus" }
  for ($i = 0; $i -lt $Bg; $i++) { SafeKeys '^t' 600; SafeGo $sites[$i] 4000 }
  if ($Bg -gt 0) { SafeKeys '^t' 600 }
  SafeGo $url 1000
  Log "[$P] krunk ($App) started with profile $Prof, $Bg background tabs, off: $Off"
}
Shot "$P-start"
