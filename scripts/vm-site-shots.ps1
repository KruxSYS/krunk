# Website hero screenshots (0.10.0, track W): Krunk in a fresh profile (no account, bookmarks or
# history of anyone), window 980x660 at the top left so the desktop's "Activate Windows" mark
# stays outside. Light: a Wikipedia article. Dark: the new tab page after visiting three sites.
# The shots are cropped to the window on the build host (scripts/site-shots.sh).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'site'
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
foreach ($theme in 'light', 'dark') {
  $profile = "$K\psite-$theme"
  Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
  Copy-Item -Recurse "$K\sp-tpl" $profile
  python -c "import sqlite3,sys; d=sqlite3.connect(sys.argv[1]); [d.execute('DELETE FROM '+t) for t in ('urls','visits') if d.execute('SELECT 1 FROM sqlite_master WHERE name=?',(t,)).fetchone()]; d.execute(""DELETE FROM kv WHERE key='session'""); d.commit()" "$profile\Krunk\browser.db" 2>&1 | Out-Null
  @{ theme = $theme; checkUpdatesAutomatically = $false } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
  Log "[$P] $theme settings: $(python $K\setkv.py $profile "@$K\patch.json")"
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $A = MainWindow
  [U]::MoveWindow($A, 0, 0, 980, 660, $true) | Out-Null; Start-Sleep 1
  if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
  if ($theme -eq 'light') {
    SafeGo 'https://en.wikipedia.org/wiki/Web_browser' 7000
    Keys '{ESC}' 500
    Shot "$P-light"
  } else {
    foreach ($u in 'https://en.wikipedia.org/wiki/Web_browser', 'https://www.bbc.com/news', 'https://github.com/') { SafeGo $u 5000 }
    SafeKeys '^t' 3000
    Shot "$P-dark"
  }
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 2
}
Log "[$P] done"
