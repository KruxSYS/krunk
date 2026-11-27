# Krunk 0.30.1 check (VM, interactive task): -Phase update = the installed 0.30.0 (real profile)
# finds 0.30.1 on the live server and updates in-app; -Phase checks = on the installed path (which
# has its firewall rule), fresh profiles: 1) launch: no UDP 5353/1900 for 60 s; 2) Cast… -> 5353 and
# the picker; 3) fresh profile, a page's PresentationRequest.start() -> discovery starts; 4) casting
# off: no sockets. Evidence: C:\pipeline\krunk\v0300\shots\run.log + R301-*.png.
param([string]$Phase = 'checks', [string]$To = '0.30.1')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$V = 'C:\pipeline\krunk\v0300'; $Shots = "$V\shots"; $Py = 'C:\Program Files\Python312\python.exe'
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'; $data = Join-Path $env:LOCALAPPDATA 'Krunk'
function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function KrunkPids { @(Get-Process Krunk -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id }) }
function PopupRectNow { $pids = KrunkPids; $pw = [TW]::All() | Where-Object { $pids -contains [TW]::Pid($_) -and [TW]::IsWindowVisible($_) -and [TW]::Cls($_) -eq 'KrunkPopup' } | Select-Object -First 1; if ($pw) { WindowRect $pw } else { $null } }
function Udp([string]$tag) { $ids = KrunkPids; $e = Get-NetUDPEndpoint -ErrorAction SilentlyContinue | ? { $ids -contains $_.OwningProcess } | % { "$($_.LocalAddress):$($_.LocalPort)" }
  Log "R301 $tag udp=[$($e -join ' ')] mdns5353=$([bool]($e -match ':5353$')) ssdp1900=$([bool]($e -match ':1900$'))"; return [bool]($e -match ':5353$') }
function Fw([string]$tag) { $fw = [U]::FindWindow([NullString]::Value, 'Windows Security'); if ($fw -ne [IntPtr]::Zero) { Log "R301 $tag firewall prompt"; Shot "R301-$tag-firewall"; Click 546 543 1500; return $true }; return $false }
$exe = Join-Path $inst 'Krunk.exe'
if ($Phase -eq 'update') {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1
  Remove-Item Env:LOCALAPPDATA -ErrorAction SilentlyContinue
  Log "R301 update: installed $(Version)"
  Start-Process -FilePath $exe -WorkingDirectory $inst
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep 4; $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 1; Focus $A | Out-Null
  for ($i = 0; $i -lt 60 -and -not (Select-String -Path "$data\krunk.log" -Pattern "$To is available" -Quiet); $i++) { Start-Sleep 1 }
  Log "R301 update: $((Get-Content "$data\krunk.log" | Select-String 'update check|is available' | Select-Object -Last 1).Line)"
  $c = ClientRect $A; Click ($c.Right - 20) 56 1200; Shot 'R301-u1-menu'
  $m = PopupRectNow; if ($m) { Click ($m.Left + 80) ($m.Top + 22) 2000 }
  $r = PopupRectNow; Shot 'R301-u2-prompt'
  if ($r) { Click ($r.Right - 66) ($r.Top + 103) 3000 } else { Log 'R301 update: no prompt' }
  $t0 = Get-Date
  for ($i = 0; $i -lt 300 -and (Version) -ne $To; $i++) { Start-Sleep 1 }
  for ($i = 0; $i -lt 60 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep 1 }
  Start-Sleep 20
  Log "R301 update: version $(Version) after $([int]((Get-Date) - $t0).TotalSeconds) s; exe $((Get-Item $exe).VersionInfo.ProductVersion); libcef $((Get-FileHash "$inst\libcef.dll").Hash.Substring(0,16))"
  Shot 'R301-u3-after'
  Log "R301 update: log $((Get-Content "$data\krunk.log" | Select-String 'release files|engine files' | Select-Object -Last 2) -join ' ## ')"
  Get-Process Krunk -ErrorAction SilentlyContinue | % { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
  return
}
if (-not (Get-NetTCPConnection -LocalPort 18112 -State Listen -ErrorAction SilentlyContinue)) { Start-Process -FilePath $Py -ArgumentList '-m','http.server','18112','--bind','127.0.0.1','--directory',$V -WindowStyle Hidden; Start-Sleep 2 }
Log "R301 checks: installed $(Version) exe $((Get-Item $exe).VersionInfo.ProductVersion)"
function StartK([string]$prof, [string]$set = '') {
  Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 2
  $P = "C:\pipeline\krunk\p0301-$prof"
  if (-not $set) { Remove-Item -Recurse -Force $P -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $P | Out-Null }
  else { $o = & $Py "$V\setprof.py" "$P\Krunk\browser.db" $set 2>&1; Log "R301 setprof $prof $set -> $o" }
  $env:LOCALAPPDATA = $P; Start-Process -FilePath $exe
  for ($i = 0; $i -lt 80 -and (MainWindow) -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500 }
  $A = MainWindow; [U]::MoveWindow($A, 0, 0, 1280, 760, $true) | Out-Null; Start-Sleep 3
  [U]::keybd_event(0x12,0,0,[UIntPtr]::Zero); [void][U]::SetForegroundWindow($A); [U]::keybd_event(0x12,0,2,[UIntPtr]::Zero); Click 640 30 600 }
# 1) launch, fresh profile: 60 s with no discovery sockets (a page is loaded meanwhile)
StartK 'c1'; $any = $false
foreach ($s in 0..6) { if (Udp "1-launch-t$($s*10)s") { $any = $true }; if ($s -eq 2) { Go 'http://127.0.0.1:18112/cast.html' 3000 }; Start-Sleep 7 }
Log "R301 1-launch: discovery in the first 60 s = $any"; Shot 'R301-1-launch-probe'
# 2) Cast… from the menu: discovery starts, the picker opens
Click 1250 56 1500; Click 1020 568 3000
if (Fw '2') { Start-Sleep 2; Click 1250 56 1500; Click 1020 568 3000 }
Start-Sleep 3; Shot 'R301-2-picker'; [void](Udp '2-after-cast-menu'); Keys '{ESC}' 800
# 3) fresh profile, a page's own request: a real click on site.html's button (page top 104 + 120..180)
StartK 'c3'; Go 'http://127.0.0.1:18112/site.html' 4000; [void](Udp '3-before-site-click')
Click 640 260 3000; Start-Sleep 3; Shot 'R301-3-site-request'; [void](Udp '3-after-site-click'); Keys '{ESC}' 800
# 4) casting off (profile c3): no sockets at all, also after loading the probe page
Get-Process Krunk -ErrorAction SilentlyContinue | % { $_.CloseMainWindow() | Out-Null }; Start-Sleep 4
StartK 'c3' 'casting=false'; Go 'http://127.0.0.1:18112/cast.html' 4000
foreach ($s in 0..2) { [void](Udp "4-off-t$($s*10)s"); Start-Sleep 10 }
Click 1250 56 1500; Shot 'R301-4-off-menu'; Keys '{ESC}' 600
Get-Process Krunk -ErrorAction SilentlyContinue | % { $_.CloseMainWindow() | Out-Null }; Start-Sleep 3; Get-Process Krunk -ErrorAction SilentlyContinue | Stop-Process -Force
Log 'R301 checks: done'
