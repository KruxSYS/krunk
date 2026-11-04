# Lists LightBrowser's process tree (the browser process and its engine
# subprocesses) with type and working set (MB). Run on the Windows VM over ssh.
#   powershell -NoProfile -ExecutionPolicy Bypass -File vm-mem.ps1
$all = Get-CimInstance Win32_Process
$root = $all | Where-Object { $_.Name -eq 'LightBrowser.exe' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
if (-not $root) { 'LightBrowser is not running'; exit 1 }
$tree = @($root); $frontier = @($root.ProcessId)
while ($frontier.Count) {
  $kids = $all | Where-Object { $frontier -contains $_.ParentProcessId }
  $tree += $kids; $frontier = @($kids | ForEach-Object { $_.ProcessId })
}
$rows = foreach ($p in $tree) {
  $type = if ($p.CommandLine -match '--type=([a-z-]+)') { $Matches[1] } else { 'browser' }
  if ($type -eq 'utility' -and $p.CommandLine -match '--utility-sub-type=([\w.]+)') { $type = 'utility:' + ($Matches[1] -split '\.')[-1] }
  [pscustomobject]@{ Pid = $p.ProcessId; Name = $p.Name; Type = $type; WorkingSetMB = [math]::Round($p.WorkingSetSize / 1MB, 1) }
}
$rows | Sort-Object Type | Format-Table -AutoSize | Out-String -Width 200
"processes=$($rows.Count) renderers=$(@($rows | Where-Object Type -eq 'renderer').Count) total_working_set_mb=$([math]::Round(($rows | Measure-Object WorkingSetMB -Sum).Sum,1))"
