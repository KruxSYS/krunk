$Theme = 'dark'
# Krunk 0.7.0 M4 (R1-R3): the operator's twelve tile titles on the new tab page at three window
# widths and at 100/125 % zoom (no horizontal scrollbar, no clipped tile); a wallpaper and the
# colour pickers changed in Settings → Appearance apply live in two windows and survive a
# restart. Fresh profile (copy of sp-tpl) with the tiles pinned in browser.db.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "ntp-$Theme"
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
# (R1) Three widths at 100 %, then 125 % (page zoom).
foreach ($w in 800, 1280, 1920) {
  [U]::MoveWindow($A, 0, 0, [Math]::Min($w, 1280), 760, $true) | Out-Null; Start-Sleep 1
  if ($w -gt 1280) { [U]::ShowWindow($A, 3) | Out-Null; Start-Sleep 1 }
  Shot "$P-1-ntp-$w"
  if ($w -gt 1280) { [U]::ShowWindow($A, 9) | Out-Null; Start-Sleep 1 }
}
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeKeys '^{+}' 600; SafeKeys '^{+}' 600
Shot "$P-2-ntp-zoom"
SafeKeys '^0' 600
[U]::MoveWindow($A, 0, 0, 800, 760, $true) | Out-Null; Start-Sleep 1
SafeKeys '^{+}' 600; SafeKeys '^{+}' 600
Shot "$P-3-ntp-800-zoom"
SafeKeys '^0' 600
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# (R2/R3) A second window shows the new tab page; Settings → Appearance in the first.
SafeKeys '^n' 2500
$windows = @(Frames)
Log "[$P] windows: $($windows.Count)"
$B = $windows | Where-Object { $_ -ne $A } | Select-Object -First 1
[U]::MoveWindow($B, 640, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
[U]::MoveWindow($A, 0, 0, 640, 760, $true) | Out-Null; Start-Sleep 1
Focus $A | Out-Null
SafeGo 'krunk://settings#appearance' 2500
Shot "$P-4-settings-appearance"
Log "[$P] done; the picker steps follow in vm-070-appearance.ps1"
Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk.log"
