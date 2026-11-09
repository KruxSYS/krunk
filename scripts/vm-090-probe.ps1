. C:\pipeline\krunk\lib.ps1
. C:\pipeline\krunk\vm-windows.ps1
$A = MainWindow; Focus $A | Out-Null
SafeGo 'krunk://settings#account' 2500
Shot 'probe-account-signed-out'
Log "[probe] done"
