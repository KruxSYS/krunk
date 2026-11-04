# YouTube ad-blocking check on the VM: opens a watch page, starts playback, takes
# screenshots over time and reads the player state from the DevTools console (ad
# showing, ad module, anti-adblock wall, playback time, stylesheet rules).
# Usage: vm-youtube.ps1 -Video ID -Tag name -Probe <SendKeys-escaped expression>
param([string]$Video, [string]$Tag, [string]$Probe)
. C:\pipeline\krunk\lib.ps1
Click 640 15 300
Go "https://www.youtube.com/watch?v=$Video" 7000
Shot "$Tag-07s"
# Start playback if it did not autoplay (click the player once).
Click 452 418 800
Start-Sleep 8
Shot "$Tag-16s"
Start-Sleep 22
Shot "$Tag-38s"
Keys '{F12}' 3500
Click 147 45 1200
Keys $Probe 300
Keys '{ENTER}' 1500
Shot "$Tag-probe"
Keys '%{F4}' 1000
