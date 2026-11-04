# Searches every file under LightBrowser's data dir (engine profile included)
# for the given strings, as ASCII and UTF-16LE. Use after saving a test login
# to check that no password or master password is written in plaintext.
#   powershell -NoProfile -ExecutionPolicy Bypass -File vm-leak-scan.ps1 -Needles 'pw1','master pw'
param([Parameter(Mandatory)][string[]]$Needles)
$root = "$env:LOCALAPPDATA\LightBrowser"
$latin1 = [Text.Encoding]::GetEncoding(28591)
$hits = 0; $scanned = 0; $bytes = 0
foreach ($f in Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue) {
  try {
    $fs = [IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite,Delete')
    $ms = New-Object IO.MemoryStream; $fs.CopyTo($ms); $fs.Close(); $data = $ms.ToArray()
  } catch { "SKIP (locked) $($f.FullName)"; continue }
  $scanned++; $bytes += $data.Length
  $text = $latin1.GetString($data)
  foreach ($n in $Needles) {
    $u16 = ($n.ToCharArray() | ForEach-Object { "$_`0" }) -join ''
    if ($text.Contains($n) -or $text.Contains($u16)) { "HIT '$n' in $($f.FullName)"; $hits++ }
  }
}
"scanned=$scanned files, $([math]::Round($bytes/1MB,1)) MB, hits=$hits"
