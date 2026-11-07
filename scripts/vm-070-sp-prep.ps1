# Krunk 0.7.0 M0 (R24): the Speedometer profiles. sp-tpl is a fresh profile that has fetched its
# filter lists; the configurations are copies with settings changed (setkv.py):
#   sp-A defaults, sp-B ad blocker off, sp-C tracking protection off (Custom, every protection
#   off), sp-D the 0.6.0 extension profile (uBlock Origin Lite + Dark Reader; the fixture off).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'sp-prep'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
$App = 'C:\pipeline\krunk\app'
Remove-Item -Recurse -Force "$K\sp-tpl" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$K\sp-tpl" | Out-Null
$env:LOCALAPPDATA = "$K\sp-tpl"
Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
for ($i = 0; $i -lt 200; $i++) {
  Start-Sleep 1
  if (Select-String -Path "$K\sp-tpl\Krunk\krunk.log" -Pattern 'filter lists checked' -Quiet -ErrorAction SilentlyContinue) { break }
}
Log "[$P] filter lists after $i s"
Start-Sleep 5
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 5
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
foreach ($c in 'A', 'B', 'C') {
  Remove-Item -Recurse -Force "$K\sp-$c.base" -ErrorAction SilentlyContinue
  Copy-Item -Recurse "$K\sp-tpl" "$K\sp-$c.base"
}
Log "[$P] B: $(python $K\setkv.py $K\sp-B.base '{\"adblockEnabled\": false}')"
Log "[$P] C: $(python $K\setkv.py $K\sp-C.base '{\"protection\": {\"level\": \"custom\", \"fingerprinting\": false, \"cryptomining\": false, \"thirdPartyCookies\": \"off\", \"stripParams\": false, \"referrer\": \"full\", \"bounceCleanup\": false}, \"blockThirdPartyCookies\": false, \"stripTrackingParams\": false}')"
Remove-Item -Recurse -Force "$K\sp-D.base" -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\p060x" "$K\sp-D.base"
python -c "import json,sys; p=sys.argv[1]; s=json.load(open(p)); [e.update(enabled=False) for e in s if e['name']=='Krunk fixture']; json.dump(s,open(p,'w'),indent=2); print([(e['name'],e['enabled']) for e in s])" "$K\sp-D.base\Krunk\extensions\state.json" | ForEach-Object { Log "[$P] D: $_" }
foreach ($c in 'A', 'B', 'C', 'D') {
  python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$K\sp-$c.base\Krunk\browser.db"
}
Log "[$P] done"
