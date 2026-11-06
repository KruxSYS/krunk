# Krunk 0.6.0 milestone 3 (R6, R7): inline autocomplete in the address bar (a visited address
# is completed and selected while typing; Enter goes there; Backspace removes the completion)
# and the address bar's context menu (Undo, Cut, Copy, Paste, Paste and go, Delete, Select all).
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$P = 'address'
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
if (-not (Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'krunk\\[s]erve\.ps1.*8094' })) {
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-File','C:\pipeline\krunk\serve.ps1','-PrefixList','http://*:8094/'
  Start-Sleep 2
}
$profile = 'C:\pipeline\krunk\p060a'
Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $profile | Out-Null
$env:LOCALAPPDATA = $profile
Start-Process -FilePath C:\pipeline\krunk\app\Krunk.exe -WorkingDirectory C:\pipeline\krunk\app
for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
Start-Sleep 6
$A = MainWindow
[U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1
if (-not (Focus $A)) { Log '[setup] could not focus A'; throw 'no focus' }
$c = ClientRect $A
function X([double]$dip) { [int]($c.Left + $dip * $c.Scale) }
function Y([double]$dip) { [int]($c.Top + $dip * $c.Scale) }
# History to complete from.
SafeGo 'https://example.com/' 3000
SafeGo 'http://localhost:8094/links.html' 2500
# (R6) "exa" completes to example.com with the tail selected; Enter goes there.
SafeKeys '^l' 300; Keys 'exa' 700
Shot "$P-1-inline-exa"
Keys '{ENTER}' 3000
Shot "$P-2-enter-went-to-example"
Log "[$P] after Enter title: $([TW]::Txt($A))"
# Typing on replaces the selection; Backspace removes the completion and does not re-add it.
SafeKeys '^l' 300; Keys 'exa' 700; Keys 'm' 500
Shot "$P-3-typed-on"
Keys '{BACKSPACE}' 600
Shot "$P-4-backspace-no-completion"
Keys '{ESC}' 300; Keys '{ESC}' 300
# A search query (with a space) is never completed.
SafeKeys '^l' 300; Keys 'exa mple' 700
Shot "$P-5-query-not-completed"
Keys '{ESC}' 300; Keys '{ESC}' 300
# Arrow keys move the highlight; Esc reverts.
SafeKeys '^l' 300; Keys 'exa' 700; Keys '{DOWN}' 400
Shot "$P-6-arrow-down"
Keys '{ESC}' 400
Shot "$P-7-esc-reverts"
Keys '{ESC}' 300
# (R7) The context menu on a selection: Copy, then Paste and go with an address in the clipboard.
Set-Clipboard -Value 'https://example.org/'
SafeKeys '^l' 300; Keys 'hello world' 400
Keys '+{HOME}' 200
RightClick (X 300) (Y 56) 1200
Shot "$P-8-context-menu"
$pw = [U]::FindWindow('KrunkPopup', [NullString]::Value); $pr = WindowRect $pw
Log "[$P] menu popup: $($pr.Left),$($pr.Top)-$($pr.Right),$($pr.Bottom)"
# Copy (third item): the clipboard gets the selection.
Click ($pr.Left + 60) ($pr.Top + [int](88 * $c.Scale)) 800
Log "[$P] clipboard after Copy: '$(Get-Clipboard -Raw)'"
Set-Clipboard -Value 'https://example.org/'
RightClick (X 300) (Y 56) 1200
Shot "$P-9-context-menu-again"
# "Paste and go" is the sixth item.
Click ($pr.Left + 60) ($pr.Top + [int](150 * $c.Scale)) 3000
Shot "$P-10-paste-and-go"
Log "[$P] after Paste and go title: $([TW]::Txt($A))"
# Paste and search with words in the clipboard: the menu says "Paste and search".
Set-Clipboard -Value 'krunk browser'
SafeKeys '^l' 300
RightClick (X 300) (Y 56) 1200
Shot "$P-11-paste-and-search-label"
Keys '{ESC}' 500
# Copying the whole unedited address copies the full URL.
SafeKeys '^l' 300; Keys '^c' 300
Log "[$P] clipboard after Ctrl+C on the address: '$(Get-Clipboard -Raw)'"
Keys '{ESC}' 300
foreach ($h in (Frames)) { Focus $h | Out-Null; SafeKeys "%{F4}" 1500 }
Start-Sleep 2
Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
Copy-Item "$profile\Krunk\krunk.log" "C:\pipeline\krunk\shots\$P-krunk.log" -ErrorAction SilentlyContinue
Log "[$P] done"
