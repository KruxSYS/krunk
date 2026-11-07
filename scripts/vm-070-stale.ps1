$Phase = 'old'; $NewSetup = 'C:\pipeline\krunk\Krunk-Setup-new.exe'
# Krunk 0.7.0 M0/M1 (R23): can a silent update leave the old engine behind? Phases:
#   old         uninstall Krunk, install 0.5.1 (the codec-less CEF "minimal" engine) from the live
#               server, record libcef.dll
#   locked-060  libcef.dll held as a mapped image (what a lingering engine process does), then the
#               0.6.0 installer /S as Krunk's updater runs it: which libcef.dll is installed?
#   zip         the 0.6.0 zip unpacked over the 0.5.1 folder with libcef.dll held the same way
#   locked-new  the same with the hardened installer ($NewSetup): moved aside and replaced
#   hard-060    libcef.dll opened without delete sharing, then the 0.6.0 installer /S
#   hard-new    libcef.dll opened without delete sharing (cannot be moved): the hardened installer
#               must stop before writing anything (exit code 3, update-error.txt, old version intact)
#   restore     uninstall the test install
# Every phase logs the installed version, libcef.dll's size and SHA-256 and the installer's exit code.
. C:\pipeline\krunk\lib.ps1
$P = "stale-$Phase"
$inst = Join-Path $env:LOCALAPPDATA 'Programs\Krunk'
$K = 'C:\pipeline\krunk'
$minimal = '07168eacd81bc09ab9ab895d39bc1a80913b09c844f0b5a4730f5f89a71133f5'
$codecs = 'c4e38bbf9f09701640956ea4adb9c28ed740d3348555f37e2de686f4721e7135'
function Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Krunk' -EA SilentlyContinue).DisplayVersion }
function Engine([string]$dir = $inst) {
  $f = Join-Path $dir 'libcef.dll'
  if (-not (Test-Path $f)) { return 'libcef.dll missing' }
  $h = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
  $kind = if ($h -eq $minimal) { 'MINIMAL (0.5.1 engine, no H.264/AAC)' } elseif ($h -eq $codecs) { 'codec engine (0.5.2+)' } else { 'unknown' }
  "libcef.dll $((Get-Item $f).Length) bytes sha256 $($h.Substring(0,16))… = $kind"
}
function Report([string]$what) {
  Log "[$P] $what | version $(Version) | $(Engine) | old-krunk files: $(@(Get-ChildItem $inst -Recurse -Filter *.old-krunk -EA SilentlyContinue).Count) | update-error.txt: $(if (Test-Path "$env:LOCALAPPDATA\Krunk\update-error.txt") { Get-Content "$env:LOCALAPPDATA\Krunk\update-error.txt" } else { 'none' })"
}
function Fetch([string]$version) {
  $out = "$K\Krunk-Setup-$version.exe"
  if (-not (Test-Path $out)) {
    Invoke-WebRequest -UseBasicParsing "https://krunk.209.wf/download/$version/Krunk-Setup-$version.exe" -OutFile $out
  }
  $sums = (Invoke-WebRequest -UseBasicParsing "https://krunk.209.wf/download/$version/SHA256SUMS").Content
  $want = ($sums -split "`n" | Where-Object { $_ -match "Krunk-Setup-$version.exe" }) -split ' ' | Select-Object -First 1
  $have = (Get-FileHash -Algorithm SHA256 $out).Hash.ToLower()
  Log "[$P] $out sha256 matches the server: $($want -eq $have)"
  $out
}
function RunSetup([string]$setup) {
  $proc = Start-Process -FilePath $setup -ArgumentList '/S' -PassThru
  $proc.WaitForExit(300000) | Out-Null
  Log "[$P] $setup /S exit code $($proc.ExitCode)"
}
# Holds libcef.dll from another process: 'image' loads it (with its own folder's DLLs) as an
# engine process does (a loaded image can be renamed, not overwritten), 'handle' opens it
# without delete sharing (can be neither).
function Hold([string]$how) {
  $f = Join-Path $inst 'libcef.dll'
  $code = if ($how -eq 'image') {
    @"
Add-Type -MemberDefinition '[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr LoadLibraryExW(string f, IntPtr h, uint flags);' -Name L -Namespace W
`$m = [W.L]::LoadLibraryExW('$f', [IntPtr]::Zero, 0x8)
Set-Content '$K\hold.txt' ('loaded at ' + `$m.ToInt64() + ' error ' + [Runtime.InteropServices.Marshal]::GetLastWin32Error())
Start-Sleep 600
"@
  } else {
    @"
`$s = [IO.File]::Open('$f', 'Open', 'Read', 'Read')
Set-Content '$K\hold.txt' ('open ' + `$s.Length)
Start-Sleep 600
"@
  }
  Remove-Item "$K\hold.txt" -EA SilentlyContinue
  # (-EncodedCommand: quotes inside -Command would not survive Start-Process.)
  $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
  $script:holder = Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-EncodedCommand', $enc -PassThru
  for ($i = 0; $i -lt 40 -and -not (Test-Path "$K\hold.txt"); $i++) { Start-Sleep -Milliseconds 500 }
  Log "[$P] holding libcef.dll ($how): $(Get-Content "$K\hold.txt" -EA SilentlyContinue)"
}
function Release { if ($script:holder) { Stop-Process -Id $script:holder.Id -Force -EA SilentlyContinue; Start-Sleep 1 } }
function Uninstall {
  Get-Process Krunk -EA SilentlyContinue | Stop-Process -Force
  if (Test-Path "$inst\uninstall.exe") {
    Start-Process "$inst\uninstall.exe" -ArgumentList '/S' -Wait
    # The uninstaller copies itself to %TEMP% and returns at once.
    for ($i = 0; $i -lt 60 -and (Test-Path "$inst\Krunk.exe"); $i++) { Start-Sleep 1 }
  }
  Remove-Item -Recurse -Force $inst -EA SilentlyContinue
}

Get-Process Krunk -EA SilentlyContinue | Stop-Process -Force
switch ($Phase) {
  'old' {
    Uninstall
    Remove-Item "$env:LOCALAPPDATA\Krunk\update-error.txt" -EA SilentlyContinue
    RunSetup (Fetch '0.5.1')
    Report 'installed 0.5.1'
  }
  'locked-060' {
    Hold 'image'
    RunSetup (Fetch '0.6.0')
    Report '0.6.0 installed over 0.5.1 while libcef.dll was mapped'
    Release
  }
  'zip' {
    $z = "$K\Krunk-win-x64-060.zip"
    if (-not (Test-Path $z)) { Invoke-WebRequest -UseBasicParsing 'https://krunk.209.wf/download/0.6.0/Krunk-win-x64.zip' -OutFile $z }
    Remove-Item -Recurse -Force "$K\zipover" -EA SilentlyContinue
    Expand-Archive -Force $z "$K\zipover-060"
    Hold 'image'
    $err = $null
    try { Copy-Item -Recurse -Force "$K\zipover-060\Krunk-win-x64\*" $inst -ErrorAction Stop } catch { $err = $_.Exception.Message }
    Log "[$P] 0.6.0 zip copied over the install folder while libcef.dll was mapped: error: $err"
    Report 'after the zip copy'
    Release
    Remove-Item -Recurse -Force "$K\zipover-060" -EA SilentlyContinue
  }
  'locked-new' {
    Hold 'image'
    RunSetup $NewSetup
    Report 'hardened installer over 0.5.1 while libcef.dll was mapped'
    Release
  }
  'hard-060' {
    Hold 'handle'
    RunSetup (Fetch '0.6.0')
    Report '0.6.0 installed over 0.5.1 while libcef.dll was open without delete sharing'
    Release
  }
  'hard-new' {
    Hold 'handle'
    RunSetup $NewSetup
    Report 'hardened installer while libcef.dll was open without delete sharing'
    Release
  }
  'restore' {
    Uninstall
    Report 'uninstalled'
  }
}
