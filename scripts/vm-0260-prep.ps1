$App = 'C:\pipeline\krunk\app260h'; $Base = 'lp-base'
# Krunk 0.26.0 page-load harness: the Krunk base profile every pass copies (like the 0.7.0
# Speedometer profiles): a fresh profile that has fetched its filter lists, QUIC off as in the
# Chrome/Edge runs (Web Page Replay speaks TCP only), no saved session.
. C:\pipeline\krunk\lib.ps1
$P = 'lp-prep'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
Remove-Item -Recurse -Force "$K\$Base" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$K\$Base" | Out-Null
$env:LOCALAPPDATA = "$K\$Base"
Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
for ($i = 0; $i -lt 200; $i++) {
  Start-Sleep 1
  if (Select-String -Path "$K\$Base\Krunk\krunk.log" -Pattern 'filter lists checked' -Quiet -ErrorAction SilentlyContinue) { break }
}
Log "[$P] filter lists after $i s"
Start-Sleep 5
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 5
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
Log "[$P] flags: $(python $K\setkv.py $K\$Base '{\"extraFlags\": \"--disable-quic\"}')"
python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$K\$Base\Krunk\browser.db"
Log "[$P] done: $((Get-ChildItem -Recurse "$K\$Base" | Measure-Object Length -Sum).Sum / 1MB) MB"
