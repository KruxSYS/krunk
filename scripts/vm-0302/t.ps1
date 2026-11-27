# Krunk 0.30.2 download smoke (VM, interactive task, after vm-0300\start.ps1 -Url .../dl.html -Dev).
#   -Phase clicks: same-tab download link, then the target=_blank download link
#   -Phase reload: F5 in the active tab        -Phase count: how many test downloads exist
param([string]$Phase = 'count')
. C:\pipeline\krunk\lib.ps1
$Shots = 'C:\pipeline\krunk\v0300\shots'
$dl = Join-Path $env:USERPROFILE 'Downloads'
function Count { @(Get-ChildItem $dl -Filter 'file*.bin' -ErrorAction SilentlyContinue).Count }
if ($Phase -eq 'clicks') {
  Log "D302 before clicks: downloads=$(Count)"
  Click 240 164 4000; Shot 'D302-1-same'
  Log "D302 after same-tab click: downloads=$(Count)"
  Click 240 284 5000; Shot 'D302-2-blank'
  Log "D302 after new-tab click: downloads=$(Count)"
} elseif ($Phase -eq 'reload') {
  Click 640 30 300; Keys '{F5}' 4000; Shot 'D302-3-reload'
  Log "D302 after F5: downloads=$(Count)"
} else {
  Log "D302 count: downloads=$(Count)"
}
