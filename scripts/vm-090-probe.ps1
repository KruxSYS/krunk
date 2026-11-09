param([string]$Url = 'krunk://sync')
. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$A = MainWindow; Focus $A | Out-Null
SafeGo $Url 2500
Shot ('probe-' + ($Url -replace '[^a-z]', ''))
Log "[probe] done"
