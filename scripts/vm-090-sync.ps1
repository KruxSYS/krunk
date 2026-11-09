# Krunk 0.9.0 (M12): sync on the real build (--dev, staging through the tunnel), together with
# the Linux test client (tools/sync-cli) driven from Linux between and during the phases.
#   setup    p090u (left at the setup step by 090-upgrade): Copy both, type the asked group,
#            tick, confirm → sync on, the first run. The passphrase and recovery key go to a test
#            file (p090u-secrets.txt, deleted by 'cleanup'), never into a log or a screenshot.
#   back     (CLI Q restored and added "From CLI") Sync now → the bookmark is on the bar.
#   approve  (CLI R asks to join) the request shows; share passwords; Approve; the 6 digits are
#            read from the page into sas.txt for the CLI; done.
#   approvewrong  the same, but the Linux side hands the CLI a wrong code: the request ends.
#   restore  a fresh profile p090r: a vault, signed in (code.txt → approved), krunk://sync →
#            Turn on → restore with the recovery key → the devices → on; the logins arrive.
#            Then the vault is locked (locked flag), CLI Q adds a login (cli-added flag): a run
#            leaves it waiting; after the unlock it arrives.
#   remove   p090r removes CLI R in krunk://sync: the keys rotate at once.
#   cleanup  the secrets file goes.
param([string]$Phase = 'setup')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = "sync090-$Phase"
$K = 'C:\pipeline\krunk'
$App = "$K\appdev090"
$master = 'Krunk-test-master-1'
$secrets = "$K\p090u\p090u-secrets.txt"
$profile = if ($Phase -in 'restore', 'remove') { "$K\p090r" } else { "$K\p090u" }
function TypeSecret([string]$s) { [System.Windows.Forms.SendKeys]::SendWait(($s -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300 }
function Win { $script:A = MainWindow; Focus $A | Out-Null; $script:c = ClientRect $A }
function StartKrunk {
  $env:LOCALAPPDATA = $profile
  Start-Process -FilePath "$App\Krunk.exe" -WorkingDirectory $App
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 5
  $w = MainWindow
  [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
  Win
}
function PageClick([int]$x, [int]$y, [int]$wait = 1500) { Click ([int]($c.Left + $x * $c.Scale)) ([int]($c.Top + (104 + $y) * $c.Scale)) $wait }
function KLog([string]$pattern) { (Get-Content "$profile\Krunk\krunk.log" -ErrorAction SilentlyContinue | Select-String $pattern | ForEach-Object { $_.Line }) -join ' ## ' }
function PageText { Set-Clipboard -Value 'empty'; SafeKeys '^a' 300; SafeKeys '^c' 400; $t = (Get-Clipboard) -join "`n"; Set-Clipboard -Value 'cleared'; Click ([int]($c.Left + 40 * $c.Scale)) ([int]($c.Top + 640 * $c.Scale)) 200; $t }
function Logins {
  python -c "import json,sys; v=json.load(open(sys.argv[1])); print('vault format', v.get('format'), '| sealed logins', len(v.get('entries') or []), '| sync part', 'sync' in v)" "$profile\Krunk\vault.json"
}
function Bookmarks {
  python -c "import sqlite3,sys; d=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro', uri=True); print([r[0] for r in d.execute('SELECT title FROM bookmarks WHERE url IS NOT NULL ORDER BY id')])" "$profile\Krunk\browser.db"
}

switch ($Phase) {
  'setup' {
    Win
    # The positive control after 090-upgrade's idle window: Turn on sync… (Settings card), then
    # Turn on in krunk://sync: the first sync request (the Linux side reads the staging log).
    SafeGo 'krunk://settings#account' 3000
    Log "[$P] positive control start $(Get-Date -Format o)"
    PageClick 435 454 3000                                    # Turn on sync…
    PageClick 329 303 7000                                    # Turn on
    Log "[$P] positive control end $(Get-Date -Format o)"
    Shot "$P-0-setup-step"
    Log "[$P] $(KLog 'sync:')"
    PageClick 321 342 1500                                    # Copy both
    $text = (Get-Clipboard) -join "`n"
    Set-Clipboard -Value 'cleared'
    $pass = ([regex]::Match($text, 'Passphrase: ([a-z-]+)')).Groups[1].Value
    $rk = ([regex]::Match($text, 'Recovery key: ([A-Z0-9-]+)')).Groups[1].Value
    Set-Content -Encoding ascii $secrets "$pass`n$rk"
    $g = $rk.Split('-'); $group = $g[$g.Length - 2]
    Log "[$P] copied: passphrase of $(($pass -split '-').Count) words, recovery key of $($g.Length) groups (kept in the test file only)"
    PageClick 306 414 300; TypeSecret $group
    PageClick 286 450 300                                     # I understand
    PageClick 320 488 9000                                    # Set up sync
    Shot "$P-1-on"
    Start-Sleep 6
    Log "[$P] $(KLog 'sync:')"
    Log "[$P] $(Logins)"
  }
  'confirm' {
    Win
    PageClick 320 488 9000                                    # Set up sync
    Shot "$P-1-on"
    Start-Sleep 6
    Log "[$P] $(KLog 'sync:')"
    Log "[$P] $(Logins)"
  }
  'back' {
    Win
    SafeGo 'krunk://sync' 3000
    PageClick 319 273 8000                                    # Sync now
    Shot "$P-1-synced"
    Log "[$P] bookmarks: $(Bookmarks)"
    Log "[$P] runs: $(KLog 'sync: run done' )"
  }
  { $_ -in 'approve', 'approvewrong' } {
    Win
    SafeGo 'krunk://passwords' 3000
    TypeSecret $master; Keys '{ENTER}' 3000                   # unlocked: passwords can be shared
    SafeGo 'krunk://sync' 3000
    Start-Sleep 8                                             # the approvals poll (3 s)
    Shot "$P-1-request"
    PageClick 286 434 300                                     # Also share my passwords
    PageClick 965 393 5000                                    # Approve
    $m = $null
    for ($i = 0; $i -lt 8 -and -not ($m -and $m.Success); $i++) {
      $m = [regex]::Match((PageText), '\b(\d{3}) (\d{3})\b')
      if (-not $m.Success) { Start-Sleep 3 }
    }
    if ($m.Success) { Set-Content -Encoding ascii "$K\sas.txt" ($m.Groups[1].Value + $m.Groups[2].Value) }
    Log "[$P] code shown: $($m.Success) (after $i reads)"
    Shot "$P-2-code"
    if ($Phase -eq 'approvewrong') {
      # The CLI was given a wrong code: it cancels; this side says so and approves nothing.
      Start-Sleep 12
      Shot "$P-3-cancelled"
      Log "[$P] page says cancelled: $((PageText) -match 'cancelled')"
    } else {
      for ($i = 0; $i -lt 40 -and -not ((Get-Content "$profile\Krunk\krunk.log" -Raw) -match 'a new device was approved'); $i++) { Start-Sleep 1 }
      Start-Sleep 2
      Shot "$P-3-approved"
    }
    Log "[$P] $(KLog 'answering|approved|approval')"
  }
  'restore' {
    Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
    Remove-Item "$K\approved", "$K\code.txt", "$K\locked", "$K\cli-added" -Force -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$profile\Krunk" | Out-Null
    Set-Content -Encoding ascii "$K\patch.json" '{"checkUpdatesAutomatically":false}'
    StartKrunk
    SafeGo 'krunk://passwords' 3000
    TypeSecret $master; Keys '{TAB}' 200; TypeSecret $master; Keys '{ENTER}' 4000   # a new vault
    SafeGo 'krunk://settings#account' 3000
    PageClick 417 260 5000                                    # Sign in
    SafeKeys '^1' 1200                                       # Settings (the link page may be in front)
    Set-Clipboard -Value 'empty'
    PageClick 430 350 800                                     # Copy code
    $code = ([regex]::Match(((Get-Clipboard) -join ' '), '\b([0-9A-Z]{4}-[0-9A-Z]{4})\b')).Groups[1].Value
    Set-Content -Encoding ascii "$K\code.txt" $code
    Log "[$P] link code read: $([bool]$code)"
    for ($i = 0; $i -lt 120 -and -not (Test-Path "$K\approved"); $i++) { Start-Sleep 1 }
    Start-Sleep 8
    Log "[$P] signed in: $(KLog 'account: signed in|account:')"
    SafeGo 'krunk://sync' 3000
    PageClick 329 303 5000                                    # Turn on → this account already syncs
    Shot "$P-1-join"
    $rk = (Get-Content $secrets)[1]
    PageClick 786 255 300                                     # Recovery key
    PageClick 792 292 300; TypeSecret $rk
    PageClick 700 337 12000                                   # Restore (Argon2id)
    Shot "$P-2-devices"
    PageClick 398 292 12000                                   # These devices keep access
    Shot "$P-3-on"
    Start-Sleep 10
    Log "[$P] runs: $(KLog 'sync: (run|passwords) done|restored|set up')"
    Log "[$P] $(Logins)"
    SafeGo 'krunk://passwords' 3000
    Shot "$P-4-passwords"
    # The vault locked: CLI Q's new login waits on the server.
    Click 1069 145 1500                                       # Lock
    Set-Content -Encoding ascii "$K\locked" '1'
    for ($i = 0; $i -lt 90 -and -not (Test-Path "$K\cli-added"); $i++) { Start-Sleep 1 }
    SafeGo 'krunk://sync' 3000
    PageClick 319 273 8000                                    # Sync now
    Log "[$P] locked run: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'sync: run done' | Select-Object -Last 1).Line)"
    Log "[$P] locked: $(Logins)"
    SafeGo 'krunk://passwords' 3000
    TypeSecret $master; Keys '{ENTER}' 3000                   # unlock → the next tick runs
    SafeGo 'krunk://sync' 2500
    PageClick 319 273 9000                                    # Sync now
    Log "[$P] after unlock: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'sync: passwords done' | Select-Object -Last 1).Line)"
    Log "[$P] unlocked: $(Logins)"
    SafeGo 'krunk://passwords' 3000
    Shot "$P-5-passwords-after-unlock"
  }
  'remove' {
    Win
    SafeGo 'krunk://sync' 3000
    Shot "$P-1-devices"
    PageClick 970 515 1500                                    # Remove (the 3rd row: CLI R)
    Shot "$P-2-confirm"
    PageClick 838 357 15000                                   # Remove (in-page confirmation)
    Shot "$P-3-removed"
    Log "[$P] $(KLog 'removed|rotated|passwords key|run done|passwords done')"
    Log "[$P] WARN/ERROR: $((Get-Content "$profile\Krunk\krunk.log" | Select-String ' WARN | ERROR ' | Measure-Object).Count) $(KLog ' WARN | ERROR ')"
    Copy-Item "$profile\Krunk\krunk.log" "$Shots\$P-krunk-p090r.log"
    Copy-Item "$K\p090u\Krunk\krunk.log" "$Shots\$P-krunk-p090u.log" -ErrorAction SilentlyContinue
  }
  'cleanup' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item $secrets, "$K\sas.txt", "$K\code.txt", "$K\approved", "$K\locked", "$K\cli-added" -Force -ErrorAction SilentlyContinue
    Log "[$P] secrets file left: $(Test-Path $secrets)"
  }
}
Log "[$P] done"
