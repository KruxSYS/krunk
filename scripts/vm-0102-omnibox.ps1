# Krunk 0.10.2: the address bar completes the site being typed from the first letter, and lists
# the site first with its pages under it. A fresh profile visits two YouTube videos (one three
# times), never the home page. Then: "y" and "yo" complete youtube.com (top row: the home page,
# then the search, then the videos), Enter opens https://www.youtube.com/; "youtube.com/w"
# completes the most visited video with its own case; "yo yo" completes nothing.
# Screenshots "om-*".
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'om'
$profile = 'C:\pipeline\krunk\p0102'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }

foreach ($u in 'https://www.youtube.com/watch?v=afI-E63m5Ag', 'https://www.youtube.com/watch?v=jNQXAC9IVRw',
               'https://www.youtube.com/watch?v=afI-E63m5Ag', 'https://www.youtube.com/watch?v=afI-E63m5Ag') {
  SafeGo $u 5000
}
Shot "$P-1-history-made"
SafeGo 'about:blank' 1500
SafeKeys '^l' 400
SafeKeys 'y' 1500
Shot "$P-2-typed-y"
SafeKeys 'o' 1500
Shot "$P-3-typed-yo"
SafeKeys '{ENTER}' 5000
Shot "$P-4-enter-opens-home"
SafeKeys '^l' 400
SafeKeys 'youtube.com/w' 1500
Shot "$P-5-typed-path"
SafeKeys '{ESC}' 300; SafeKeys '{ESC}' 300
SafeKeys '^l' 400
SafeKeys 'yo yo' 1500
Shot "$P-6-words-no-completion"
SafeKeys '{ESC}' 300; SafeKeys '{ESC}' 300
foreach ($f in Frames) { if (Focus $f) { SafeKeys '%{F4}' 1500 } }
Start-Sleep 3
Log "[end] Krunk processes: $(@(Get-Process Krunk -ErrorAction SilentlyContinue).Count)"
