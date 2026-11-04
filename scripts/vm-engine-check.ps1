# Checks which browser engine LightBrowser runs on. For every LightBrowser.exe process (the browser
# and its CEF subprocesses), it reports whether libcef.dll, WebView2Loader.dll or
# EmbeddedBrowserWebView.dll is loaded. It also lists any msedgewebview2.exe whose parent chain leads
# to a LightBrowser process (expected: none; Windows itself runs msedgewebview2 for widgets and similar).
#   powershell -NoProfile -ExecutionPolicy Bypass -File vm-engine-check.ps1
$all = Get-CimInstance Win32_Process
$ours = @($all | Where-Object { $_.Name -eq 'LightBrowser.exe' })
if (-not $ours) { 'LightBrowser is not running'; exit 1 }
$rows = foreach ($p in $ours) {
  $type = if ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { 'browser' }
  $mods = try { (Get-Process -Id $p.ProcessId -ErrorAction Stop).Modules | ForEach-Object { $_.ModuleName.ToLower() } } catch { @('<no access>') }
  [pscustomobject]@{
    Pid = $p.ProcessId; Type = $type
    libcef = [bool]($mods -contains 'libcef.dll')
    WebView2Loader = [bool]($mods -contains 'webview2loader.dll')
    EmbeddedBrowserWebView = [bool]($mods -contains 'embeddedbrowserwebview.dll')
  }
}
$rows | Sort-Object Type | Format-Table -AutoSize | Out-String -Width 200
$ourIds = @($ours | ForEach-Object { $_.ProcessId })
$byId = @{}; foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }
function From-Us($p) {
  $seen = 0
  while ($p -and $seen -lt 64) { if ($ourIds -contains $p.ProcessId) { return $true }; $p = $byId[[int]$p.ParentProcessId]; $seen++ }
  return $false
}
$edge = @($all | Where-Object { $_.Name -eq 'msedgewebview2.exe' })
$edgeOurs = @($edge | Where-Object { From-Us $_ })
"processes=$($rows.Count) with_libcef=$(@($rows | Where-Object libcef).Count) with_webview2_modules=$(@($rows | Where-Object { $_.WebView2Loader -or $_.EmbeddedBrowserWebView }).Count)"
"msedgewebview2_system_wide=$($edge.Count) msedgewebview2_started_by_lightbrowser=$($edgeOurs.Count)"
