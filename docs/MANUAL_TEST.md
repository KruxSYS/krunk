# Manual test checklist (Windows 10/11 x64)

*Draft; the complete checklist (all screenshots, RAM measurements) comes with packaging.*

## Popup window
1. At 100 % display scaling, open the ⋮ menu. It appears directly under the button, right-aligned,
   fully on screen, with all items visible (down to "Exit").
2. Set Windows display scaling to 150 % (Settings → System → Display → Scale) and restart
   LightBrowser. Repeat step 1: the popup is still aligned under the button and is not cut off.
3. Put the window near the right and bottom edges of the screen and open the menu. The popup
   stays inside the monitor work area. Repeat on a second monitor if you have one, including a
   monitor to the left of the primary one.
4. Dismissal: with the menu open, check each of these closes it:
   - Escape
   - clicking in the page
   - clicking in the address bar
   - moving the window
   - resizing it
   - minimizing it
   - Ctrl+Tab
   - closing the tab
   - Alt+Tab to another app
5. Shield panel: it opens with keyboard focus (Tab reaches its button, Enter activates it).
   Clicking the page closes it.
6. Password prompt after a tab switch (once the vault exists):
   - Trigger "Save password?" on site A, then press Ctrl+Tab. The prompt disappears and nothing
     is saved.
   - Trigger it again, switch to another tab and back, and confirm the prompt is gone and
     nothing was saved.
   - Fill prompt on site A: navigate the tab to site B before choosing an account. Nothing is
     filled.
