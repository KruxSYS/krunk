# Lists LightBrowser's process tree with type and working set (MB), and the
# WebView2 browser process command line. Run on the Windows VM over ssh.
param([switch]$CommandLine)
$all = Get-CimInstance Win32_Process
$root = $all | Where-Object { $_.Name -eq 'LightBrowser.exe' } | Select-Object -First 1
if (-not $root) { 'LightBrowser is not running'; exit 1 }
$tree = @($root); $frontier = @($root.ProcessId)
while ($frontier.Count) {
  $kids = $all | Where-Object { $frontier -contains $_.ParentProcessId }
  $tree += $kids; $frontier = @($kids | ForEach-Object { $_.ProcessId })
}
$rows = foreach ($p in $tree) {
  $type = if ($p.CommandLine -match '--type=([a-z-]+)') { $Matches[1] } elseif ($p.Name -eq 'LightBrowser.exe') { 'host' } else { 'browser' }
  if ($type -eq 'utility' -and $p.CommandLine -match '--utility-sub-type=([\w.]+)') { $type = 'utility:' + ($Matches[1] -split '\.')[-1] }
  [pscustomobject]@{ Pid = $p.ProcessId; Type = $type; WorkingSetMB = [math]::Round($p.WorkingSetSize / 1MB, 1) }
}
$rows | Sort-Object Type | Format-Table -AutoSize | Out-String -Width 200
"processes=$($rows.Count) renderers=$(@($rows | Where-Object Type -eq 'renderer').Count) total_working_set_mb=$([math]::Round(($rows | Measure-Object WorkingSetMB -Sum).Sum,1))"
if ($CommandLine) {
  $b = $tree | Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
  "BROWSER COMMAND LINE:"; $b.CommandLine
}
