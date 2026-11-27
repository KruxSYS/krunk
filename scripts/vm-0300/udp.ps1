# The UDP sockets of every Krunk process (browser, network service, …): mDNS (5353) and SSDP
# show up here once the Media Router searches the LAN. Appends to shots\run.log.
param([string]$Tag = 'udp')
$ids = (Get-Process Krunk -ErrorAction SilentlyContinue).Id
$eps = Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $ids -contains $_.OwningProcess } |
  Sort-Object LocalPort | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }
$line = "$(Get-Date -Format HH:mm:ss.fff) $Tag udp=[$($eps -join ' ')] mdns5353=$([bool]($eps -match ':5353$'))"
Add-Content 'C:\pipeline\krunk\v0300\shots\run.log' $line
$line
