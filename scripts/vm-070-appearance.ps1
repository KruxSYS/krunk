$Theme = 'dark'
# Krunk 0.7.0 M4 (R2, R3): Settings → Appearance in one window, the new tab page in a second one.
# A toolbar and a tab-strip colour from the swatches, a typed hex colour and the colour dialog,
# then a built-in wallpaper: every change shows in both windows at once (live) and is still there
# after a restart. Fresh profile (copy of sp-tpl) with the operator's tiles pinned.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "appearance-$Theme"
$K = 'C:\pipeline\krunk'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
$profile = "$K\p070n"
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
Copy-Item -Recurse "$K\sp-tpl" $profile
$py = @'
import sqlite3, sys
d = sqlite3.connect(sys.argv[1])
d.execute("DELETE FROM kv WHERE key='session'")
tiles = [("https://plique.example/", "Home \u2014 PLIQUE DMA"), ("https://www.youtube.com/", "YouTube"),
  ("https://perfectaim.example/", "PerfectAim - Undetected Game Cheats"), ("https://azeroth.example/", "Azeroth Survivors"),
  ("https://login.example/", "Login"), ("https://app.plex.tv/", "Plex"),
  ("https://duckduckgo.com/?q=perfectaim", "perfectaim at DuckDuckGo"), ("https://canyoublockit.com/", "Can You Block It ? - A Simple Ad Block Tester"),
  ("https://www.youtube.com/feed", "YouTube"), ("https://adblock-tester.com/", "Ad Blocker Test | Check if Your Ad Blocker Works"),
  ("https://rabbitrun.example/", "RabbitRun"),
  ("https://forums.plex.tv/t/solved", "SOLVED New Media Server or New Firefox version causing Error Playing Video in Browser | Plex Forum")]
for i, (u, t) in enumerate(tiles):
    d.execute("INSERT OR REPLACE INTO tiles(url, title, state, position) VALUES (?, ?, 1, ?)", (u, t, i))
d.commit()
print(len(tiles))
'@
Set-Content -Encoding utf8 "$K\seed-tiles.py" $py
Log "[$P] tiles seeded: $(python "$K\seed-tiles.py" "$profile\Krunk\browser.db")"
@{ theme = $Theme } | ConvertTo-Json | Set-Content -Encoding ascii "$K\patch.json"
Log "[$P] settings: $(python $K\setkv.py $profile "@$K\patch.json")"
$env:LOCALAPPDATA = $profile
function StartKrunk {
  Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $script:A = MainWindow
  if (-not (Focus $A)) { Log "[$P] could not focus"; throw 'no focus' }
}
StartKrunk
SafeKeys '^n' 2500
$B = @(Frames) | Where-Object { $_ -ne $A } | Select-Object -First 1
[U]::MoveWindow($B, 640, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
[U]::MoveWindow($A, 0, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'krunk://settings#appearance' 3000
Shot "$P-1-before"
# Swatches (screen coordinates of the 640-px window at 0,0): toolbar white, tab strip indigo.
Click 354 606 1200
Shot "$P-2-toolbar-white"
Click 522 693 1200
Shot "$P-3-strip-indigo"
# A typed hex value.
Click 386 643 300
SafeKeys '^a' 100; SafeKeys '#7c2d12{ENTER}' 1500
Shot "$P-4-toolbar-typed"
# The colour dialog (Blink's own picker) for the accent.
Click 558 519 1500
Shot "$P-5-colour-dialog"
Keys '{ESC}' 600
# A wallpaper: the new tab page background card, found with the settings search.
Focus $A | Out-Null
Click 432 146 300
SafeKeys 'wallpaper' 1500
Shot "$P-6-search-wallpaper"
Click 290 379 1500
Shot "$P-7-wallpaper-aurora"
Click 290 477 1500
Shot "$P-8-wallpaper-dunes"
Log "[$P] stored: $(python -c "import sqlite3,json,sys; d=sqlite3.connect(sys.argv[1]); s=json.loads(d.execute('SELECT value FROM kv WHERE key=?',('settings',)).fetchone()[0]); print(json.dumps({'appearance': {k: s['appearance'][k] for k in ('accent','toolbarColor','stripColor')}, 'ntp': {k: s['ntp'][k] for k in ('background','wallpaper')}}))" "$profile\Krunk\browser.db")"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
# Restart: the choices are still there (both windows come back from the session).
Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
Start-Sleep 4
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
StartKrunk
Start-Sleep 3
Shot "$P-9-after-restart"
