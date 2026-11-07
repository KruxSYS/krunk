# Krunk 0.7.0 M10 (R25): Firefox 159's logins.json with entries that have no `hostname` (copies of
# the real fake login: one with the address only in formSubmitURL, one with no address at all, one
# that is not an object). Phases: setup (the file changed, a fresh Krunk with a vault, the import
# rows shown), run (the Firefox row's Import at $Bx,$By; the summary), restore (the original file).
param([string]$Phase = 'setup', [int]$Bx = 0, [int]$By = 0)
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$K = 'C:\pipeline\krunk'
$P = "import-$Phase"
$ff = Get-ChildItem "$env:APPDATA\Mozilla\Firefox\Profiles" -Directory | Where-Object { Test-Path "$($_.FullName)\logins.json" } | Select-Object -First 1
$logins = "$($ff.FullName)\logins.json"
$backup = "$K\ff-logins-backup.json"
$profile = "$K\p070imp"
$env:LOCALAPPDATA = $profile
switch ($Phase) {
  'setup' {
    Get-Process Krunk, firefox -ErrorAction SilentlyContinue | Stop-Process -Force
    if (-not (Test-Path $backup)) { Copy-Item $logins $backup }
    $j = Get-Content $backup -Raw | ConvertFrom-Json
    $orig = $j.logins[0]
    $noHost = $orig | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    $noHost.PSObject.Properties.Remove('hostname')
    $noHost.formSubmitURL = 'http://nohost.localhost:8094'
    $noHost.id = 90; $noHost.guid = '{00000000-0000-4000-8000-000000000090}'
    $nowhere = $orig | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    $nowhere.PSObject.Properties.Remove('hostname')
    $nowhere.formSubmitURL = $null; $nowhere.httpRealm = 'Intranet'
    $nowhere.id = 91; $nowhere.guid = '{00000000-0000-4000-8000-000000000091}'
    $list = @($j.logins) + @($noHost, $nowhere)
    $json = (@{ nextId = 92; logins = $list; version = 3 } | ConvertTo-Json -Depth 6)
    # One entry that is not an object at all.
    $json = $json -replace '"logins":\s*\[', '"logins": [ "not an entry",'
    Set-Content -Path $logins -Value $json -Encoding UTF8
    Log "[$P] Firefox logins.json: $($j.logins.Count) original + 2 without hostname + 1 not an object ($($ff.Name))"
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $profile | Out-Null
    Start-Process -FilePath "$K\app\Krunk.exe" -WorkingDirectory "$K\app"
    for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep 5
    $w = MainWindow; [U]::MoveWindow($w, 0, 0, 1280, 760, $true) | Out-Null; Focus $w | Out-Null
    SafeGo 'krunk://passwords' 3000
    Keys 'import test master' 200; Keys '{TAB}' 200; Keys 'import test master' 200; Keys '{ENTER}' 4000
    SafeGo 'krunk://settings#passwords' 4000
    Shot "$P-1-rows"
  }
  'run' {
    Focus (MainWindow) | Out-Null
    Click $Bx $By 9000
    Shot "$P-1-summary"
    Log "[$P] log: $((Get-Content "$profile\Krunk\krunk.log" | Select-String 'import') -join ' ## ')"
    SafeGo 'krunk://passwords' 3000
    Shot "$P-2-passwords"
  }
  'restore' {
    Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
    if (Test-Path $backup) { Copy-Item -Force $backup $logins; Remove-Item $backup }
    Log "[$P] Firefox logins.json restored"
  }
}
Log "[$P] done"
