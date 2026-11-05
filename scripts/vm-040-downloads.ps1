# Krunk 0.4.0 milestone 5: the downloads page shows the full address and when a download
# started and finished, and its rows have a context menu (copy link / page address, download
# again, delete file). Fresh profile; serves vm-www/8092 (dl.html and files/*.bin, made here).
# Screenshots "dl-*"; the clipboard and the file on disk are checked in run.log.
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'dl'
# The row menu's items (CSS px below the click).
$Menu = @{ Open = 21; ShowInFolder = 51; CopyLink = 92; CopyPage = 122; Again = 163; Remove = 193; Delete = 223 }

$files = 'C:\pipeline\krunk\www\8092\files'
New-Item -ItemType Directory -Force $files | Out-Null
if (-not (Test-Path "$files\small.bin")) {
  $b = New-Object byte[] (1MB); (New-Object Random 7).NextBytes($b); [IO.File]::WriteAllBytes("$files\small.bin", $b)
}
if (-not (Test-Path "$files\big.bin")) {
  $b = New-Object byte[] (64MB); (New-Object Random 9).NextBytes($b); [IO.File]::WriteAllBytes("$files\big.bin", $b)
}
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8092' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://localhost:8092/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p040d'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$dlDir = "$profile\Downloads"
New-Item -ItemType Directory -Force $dlDir | Out-Null
# A settings file that downloads into the test folder.
New-Item -ItemType Directory -Force "$profile\Krunk" | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
$page = [int]($c.Top + 104 * $c.Scale)

SafeGo 'http://localhost:8092/dl.html' 2500
# The small file, from that page (the tab stays on it: the page address is known).
SafeGo 'http://localhost:8092/files/small.bin' 4000
SafeKeys '^j' 2500
if ((Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'small*.bin').Count -eq 0) { Log '[setup] no download'; Shot "$P-x-no-download"; Get-Process Krunk | Stop-Process -Force; throw 'no download' }
Shot "$P-1-page"
# Right-click the first row's name.
$rx = [int]($c.Left + 300 * $c.Scale); $ry = [int]($page + 95 * $c.Scale)
RightClick $rx $ry 800
Shot "$P-2-menu"
Click ($rx + 60) ($ry + $Menu.CopyLink) 800
Log "[copy-link] clipboard: $(Get-Clipboard)"
RightClick $rx $ry 800
Click ($rx + 60) ($ry + $Menu.CopyPage) 800
Log "[copy-page] clipboard: $(Get-Clipboard)"
# Download again: a second entry.
RightClick $rx $ry 800
Click ($rx + 60) ($ry + $Menu.Again) 4000
Shot "$P-3-downloaded-again"
# Delete the file of the newest entry (confirm with Enter).
$newest = Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'small*.bin' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Log "[delete] newest file before: $($newest.FullName) exists=$(Test-Path $newest.FullName)"
RightClick $rx $ry 800
Click ($rx + 60) ($ry + $Menu.Delete) 1000
Shot "$P-4-confirm-delete"
Keys '{ENTER}' 1500
Log "[delete] after: exists=$(Test-Path $newest.FullName)"
Shot "$P-5-after-delete"
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
# Clean up the test's downloads in the user's Downloads folder.
Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'small*.bin' | ForEach-Object { Log "[cleanup] removing $($_.Name)"; Remove-Item $_.FullName }
Get-Content "$profile\Krunk\krunk.log" | Select-String 'download|WARN|ERROR' | ForEach-Object { Log "[log] $_" }
