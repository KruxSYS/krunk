# Krunk for Android changelog

What changed in each Krunk for Android release, newest first. The Windows browser's list is
`CHANGELOG.md`.

## Unreleased

- **Krunk is now a KruxSYS project, with a new icon.** The launcher icon and the Android TV banner
  show Krunk's "K" in the KruxSYS style (a dark square with cyan corners), and Settings › About
  names KruxSYS (https://kruxsys.com) as Krunk's publisher.

## 0.13.1 — 2026-11-23

- **Krunk's website moved to krunk.kruxsys.com.** Updates, signing in and sync now use the new
  address; the old address no longer answers. Earlier versions can't find this update by
  themselves: download it from https://krunk.kruxsys.com.

## 0.13.0 — 2026-11-21

- **Strong passwords when you sign up.** Tap the password field of a sign-up or change-password
  form and Krunk offers a strong password it made up for you. **Use password** fills it into the
  field and into the "repeat password" field; **Show** lets you see it first. When you send the
  form, Krunk asks to save the new password in your vault (or to create the vault). You can switch
  the suggestions off in Passwords › Settings › **Suggest strong passwords**.
- **Asked to save more reliably.** Krunk now asks to save a new sign-in when the site moves you on
  to another part of the same site afterwards (for example from `login.example.com` to
  `app.example.com`), and also when the vault was busy at that moment. A suggested password you
  used is always offered for saving, even on sites that send the form in the background.

## 0.12.0 — 2026-11-19

- **Krunk on your TV.** Krunk now installs and runs on Android TV and Google TV (Android 8 and
  later) and shows up in the TV's app row with its own banner. Everything works with the remote
  alone: the address bar, tabs, the menu, the new tab page, all of Settings (Passwords, VPN,
  Account & sync, Downloads, History, Bookmarks) and its dialogs. The element you are on is
  marked with a thick outline that you can see from the sofa, and Krunk keeps its buttons inside
  the TV's safe area.
- **Browsing with the remote.** On a page, **Up/Down** scroll, **Left/Right** jump to the previous
  or next link or field, and **Center** opens it (on a text field it brings up the TV keyboard).
  **Up** at the top of the page, or the **Menu** key, takes you back to the address bar; **Down**
  from there goes back into the page. **Back** goes back a page; on the first page it moves to
  the address bar first.
- **Cursor mode.** Many sites can't be used with arrow keys alone. Hold **Center** (or pick
  **Cursor mode** in the menu) to get a pointer: the arrows move it, **Center** clicks, and the page
  scrolls when the pointer reaches an edge. Hold **Center** again or press **Back** to leave it.
- **For 32-bit TVs.** There is now an `armeabi-v7a` APK as well, for devices such as the
  Chromecast with Google TV. The update check picks the right file for each device by itself.
- Phones and tablets work exactly as before. On a TV, things a TV can't do stay out of the way:
  Share is hidden, fingerprint unlock isn't offered, and a missing file picker or settings page
  gets a short message instead of an error.

## 0.11.1 — 2026-11-19

- **Fingerprint unlock, offered right away.** After you make your password vault (in
  Passwords, or when you turn on sync), Krunk asks whether you want to unlock it with your
  fingerprint. **Use fingerprint** switches it on with one touch; **Not now** leaves it off,
  and you can still switch it on later in Passwords › Settings. You only see the question when
  your phone has a fingerprint set up.

## 0.11.0 — 2026-11-17

- **Signed in to Krunk, signed in to the website.** While Krunk is signed in to your Krunk
  account (with or without sync), opening krunk.kruxsys.com signs the website in for you: no password,
  no code to type. The website lists it under your signed-in browsers as "Signed in by" this
  phone.
- **Signing out stays signed out.** If you sign out on the website, Krunk leaves it signed out
  until you restart the app. Private tabs never sign the website in.
- The website still asks for your password before sensitive changes, such as changing your
  password or deleting your account.
- **New account from the sign-in page.** Settings › Account › **Sign in** opens the website's
  sign-in page; **Create an account** there now signs this phone in once you confirm your
  email address. Krunk keeps waiting for that confirmation for up to an hour instead of giving up
  after 10 minutes.

## 0.10.0 — 2026-11-17

- **A VPN for Krunk only.** Settings › Privacy & security › **VPN** imports a WireGuard (`.conf`)
  or OpenVPN (`.ovpn`) file from your VPN provider: pick it with **Import profile**, or share or
  open it from your file manager or mail app. Krunk shows what it found, and any option it will
  ignore, before saving. Turn on **Use the VPN** and Krunk's pages, private tabs, downloads and
  its own requests (sync, updates, site icons, search suggestions) go through your VPN. Other
  apps are not affected: no system VPN, no key icon.
- **Nothing slips out.** While the VPN is on but not connected, Krunk loads nothing and shows
  **VPN not connected** with Retry and a way to the VPN settings. Nothing goes around the VPN.
  An address your profile does not cover shows **Not reachable through this VPN**.
- **Everywhere or private tabs only.** With **Private tabs only**, only private tabs (and what
  Krunk fetches for them) use the VPN. **Connect when Krunk starts** connects before the first
  page loads; **Reach the local network directly** lets your router and printers through.
- **See it at a glance.** A mark next to the shield shows the VPN's state (green connected,
  amber connecting, red not connected). Tap it for the profile, your address in the tunnel, the
  traffic, Connect / Disconnect and the scope. **Show the VPN in the address bar** hides it.
- Switching between Wi-Fi and mobile data reconnects the VPN at once. Profiles are stored
  encrypted with a key kept by Android, are never synced and stay out of backups.
- While the VPN is on, DNS prefetching, DNS over HTTPS, HTTP/3 and WebRTC's direct connections
  are off, so nothing leaves outside the tunnel.

## 0.9.0 — 2026-11-17

- **See who a site's certificate belongs to.** Tap the shield or the lock in the address bar:
  under the blocking part, **Connection** says whether the connection is secure and shows the
  certificate: who it was issued to and by, when it is valid, its SHA-256 fingerprint (long-press
  to copy) and the names it covers. **Certificate details** adds the serial number, the
  algorithms and the SHA-1 fingerprint. The lock opens the sheet right at this part.
- **Forget a certificate you accepted.** On a site where you chose "Continue anyway", the sheet
  says so plainly and offers **Forget this exception**; the page reloads and the warning is back.
- **Plain http and mixed pages say so.** An http page shows "Connection is not secure"; an https
  page that loaded parts without encryption says that too.
- The sheet now also opens on sites the blocker doesn't judge, such as an IP address.

## 0.8.0 — 2026-11-16

- **Back returns to the tab that opened the link.** A link opened in a new tab (a link that opens
  a new window, or "Open in new tab") closes on Back once it has no earlier page, and you are
  back on the page you came from, even after switching tabs in between, as long as that tab is
  still open (until Krunk is restarted). A link opened from another app closes on Back and
  returns you to that app.
- **Back twice to leave.** With nothing left to go back to, Back shows "Press back again to exit";
  only a second Back within 2 seconds leaves Krunk. Your tabs stay as they are.
- **History per device.** The History screen opens on **This device**; chips show each of your
  other synced devices by name, **Unknown device** (visits synced before this version) and
  **All**. Search works inside the chosen chip, and under All each entry names its device.
  Clearing This device's history removes it on your other devices too; another device's is only
  hidden on this phone. Address suggestions still use every device's history.

## 0.7.4 — 2026-11-16

- **The address bar completes what you type.** Type the start of a site you have visited
  ("exa") and the rest appears highlighted ("example.com"); press Go or Enter to open it, keep
  typing to narrow it, or delete a letter to type something else.
- **Fingerprint unlock asks by itself.** With fingerprint unlock on, unlocking your passwords
  shows the fingerprint prompt at once; "Use fingerprint" is still there if you cancel it.

## 0.7.3 — 2026-11-15

- **Sync works again with a big history.** Sync stopped with "Something went wrong with sync"
  once the history held more than 200 pages of one site: Krunk then failed to find a page it
  already had, tried to add it twice and gave up, on every run. It finds it now.

## 0.7.2 — 2026-11-15

- **Sync works again after the account's keys change.** Since a device switched sync to the
  vault password, this phone said "Krunk couldn't reach krunk.kruxsys.com" when it synced or added
  passwords: it refused one of its own requests (an address with a comma in it) and called that
  "offline". Nothing ever reached the server.
- **No more long waits.** When sync has to wait (too many attempts, the server busy), it waits a
  minute at most, never an hour.

## 0.7.1 — 2026-11-15

- **The address bar shows the whole address**, like Firefox: the site's name in the normal
  colour and the rest of the address (the page's path) in grey after it. When the address is too
  long, the end of the path is cut, never the site's name.

## 0.7.0 — 2026-11-15

- **Google is the default search engine and homepage** for new installs. Phones still on the old
  defaults switch to Google once.
- The tab count next to the address bar is centred in its box.
- "Add shortcut" is centred in its tile.
- Switches show their knob when off too, in light and dark themes.

## 0.6.2 — 2026-11-15

- **One password for sync: your vault password.** To sync this phone, sign in to your Krunk
  account and type your vault password (the one on your PC). Bookmarks, history, tabs, settings
  and passwords all come along. If the phone has no password vault yet, that same password makes
  it. No sync passphrase, no recovery key, no codes.
- **A phone that syncs without your passwords:** type your PC's vault password under Passwords in
  Account & sync, or simply unlock this phone's vault with it. Your PC must run Krunk 0.21.1 and
  have its passwords unlocked once after updating.
- Errors show right under the button you pressed.

## 0.6.1 — 2026-11-15

Passwords that didn't come along with sync, and a way past bad certificates. It installs over
0.6.0 and keeps everything.

- **Passwords sync after all.** A phone that joined sync while its vault was locked or not yet
  created never got the passwords key, so its vault stayed empty. Settings › Account & sync now
  says whether passwords sync on this phone, and if not, why: vault locked, no vault yet, not
  added on this phone, or switched off. **Add passwords** fixes the last case with your sync
  passphrase or recovery key; a wrong one changes nothing. Joining now tells you to create or
  unlock the vault first to bring passwords along.
- **Continue past a certificate warning.** When a site's certificate can't be trusted
  (self-signed, expired, issued for another name, unknown authority), Krunk shows the site, the
  reason and the certificate's SHA-256 fingerprint, with **Back to safety** and
  **Continue anyway**:
  - **Remember for this site** keeps the choice for that site and that exact certificate. If
    the site later shows a different certificate, Krunk warns again. Without it, the choice
    lasts until Krunk is closed;
  - in a private tab the choice always lasts only until Krunk is closed;
  - sites that demand a valid certificate (HSTS) get no Continue button;
  - Settings › Privacy › **Certificate exceptions** lists what you remembered; Remove one and
    Krunk warns again.
  The engine doesn't hand the certificate of a failed load to the app, so Krunk makes one extra
  connection to the site to read it (nothing is sent over it).
- Settings › Site data and Certificate exceptions no longer sometimes fail to load right after
  Krunk starts.
- Tested on an Android 16 emulator. Not tested on real phones.

## 0.6.0 — 2026-11-15

Your Krunk account and sync on the phone. It installs over 0.5.0 and keeps tabs, bookmarks,
history, settings, the password vault and downloads.

- **Sign in** (Settings › Account & sync) to the Krunk account you use on Windows. Krunk shows a
  code and opens the approval page in a tab; you approve this phone there with your account
  password. Nothing is sent to krunk.kruxsys.com before you tap Sign in.
- **Turn on sync** and get the keys in one of three ways:
  - from a device that already syncs: Windows shows 6 digits, you type them on the phone;
  - with your sync passphrase or recovery key: a wrong one is refused with a clear message, and
    nothing changes on the phone;
  - for an account that doesn't sync yet, the phone is the first device: Krunk shows the
    passphrase and the recovery key once, and asks you to type back part of the key.
- **What syncs:** the same as on Windows, encrypted on the phone before it leaves:
  - bookmarks, history, settings, open tabs and new tab pins;
  - ad-blocker exceptions and device names;
  - passwords and the never-save list, while your vault is unlocked.
  The first sync merges what the phone already has. Phone-only settings (toolbar position,
  text size) stay on the phone.
- **The status** shows your address, the last sync and how it went, waiting changes, a switch
  per kind of data, **Sync now** and the open tabs of your other devices. Sync runs while Krunk
  is open (a minute after start, every 5 minutes, 30 seconds after a change), never in the
  background.
- **Conflicts** are settled as on Windows, by the same code: bookmarks merge per field, history
  is the union of visits, settings and logins take the later change.
- **Removed on another device or on the website?** Krunk says so, stops syncing and deletes this
  phone's sync keys and sign-in. Your data on the phone stays.
- **Sign out** keeps your data, or also removes what came from sync, as you choose.
- **Stored where:** the sign-in and sync's keys are in the app's private storage, wrapped by a key
  of the phone's key store. They are in no backup and no phone-to-phone transfer: a new phone
  signs in again. The passwords key lives only inside the vault.
- Tested end to end against a test copy of the server, with Windows Krunk on a Windows VM and an
  Android 16 emulator. Not tested on real phones.
- Not on the phone yet: approving or removing other devices, "Finish securing" after a removal,
  and signing in by scanning the account page's QR code. Use Windows or the website for those.

## 0.5.0 — 2026-11-14

Updates from inside the app, for the APK from the website. It installs over 0.4.0 and keeps tabs,
bookmarks, history, settings, the password vault and downloads.

- **Coming from 0.4.0 or older? Install this APK over it once.** Those versions cannot check
  for updates, so the step to 0.5.0 is the same as before: download the APK from
  krunk.kruxsys.com and install it over the installed Krunk. Your data stays. From 0.5.0 on the app
  can fetch later versions itself.
- **Check for updates** (Settings › About). Krunk contacts krunk.kruxsys.com only when you tap it,
  and fetches two small files: the update information and its signature. If a newer version is
  there, Krunk shows it with its notes; **Download and install** fetches the one APK for your
  device, with progress and Cancel. Nothing is checked at start-up, on a timer or in the
  background. The requests carry the User-Agent `Krunk-Android/0.5.0` and nothing else of
  yours (no identifiers, no cookies).
- **Checked before Android sees it.** The update information must carry a valid signature of
  Krunk's release key, which is built into the app (the key that also signs the Windows
  updates). The APK must match the size and SHA-256 named there, be Krunk, be exactly the version
  named, and be signed with the same certificate as the Krunk you have. If any check fails Krunk
  says which one, deletes the file and installs nothing. Then Android's installer asks you; the
  first time Android also asks whether Krunk may install apps, and Krunk explains why first.
  The downloaded file is deleted after the hand-over, on any failure and on cancel.
- **Same version or older is "up to date":** Krunk never offers a downgrade.
- **A Google Play build** exists for the first time (built, not published): it has no update
  check, no update screen and no permission to install apps, because Play updates the app
  itself. Whether and when it goes to Play is still open.
- QA ran on emulator images only (Android 16 and Android 10), not on real devices.

## 0.4.0 — 2026-11-14

Downloads of Krunk's own, site permissions and site data. It installs over 0.3.0 and keeps
tabs, bookmarks, history, settings and the password vault.

- **Downloads are Krunk's own.** Until now Krunk handed a download to Android's download
  manager, which asked the server for the file a second time, without the page's cookies. Now
  Krunk saves the answer the page itself received:
  - a file behind a sign-in is the file, not the sign-in page;
  - files a page makes itself (`blob:` and `data:` links, "Export" buttons) are saved;
  - a download a page starts still asks first, with the cleaned file name and the site; the
    Download button reacts only after half a second, so a page cannot catch a tap with it;
  - files go to the device's Downloads folder (Android 10 and later; no storage permission is
    asked), and a file appears there only when it is complete.
- **Downloads screen** (⋮ menu › Downloads): running and finished downloads with their size,
  speed and time left; open, share, pause, resume, retry, restart, cancel, remove from the list,
  clear the finished ones, search. A notification shows the progress and the result (tap a
  finished one to open the file); downloads go on while Krunk is in the background.
- **Pause, resume and retry** where the server allows it (it must name the file's size and
  version and answer range requests): Krunk asks for the rest of the same version of the file,
  with your cookies. Where the server does not allow it, Krunk says the download cannot be
  paused, and a failed one offers Restart instead of starting over by itself. Files a page made
  itself cannot be continued: download them again from the page.
- **If Krunk is closed during a download**, the next start shows it as paused (it can go on) or
  failed, never as finished. Android limits downloads in the background to six hours a day;
  when that is used up, Krunk pauses what can go on and says so.
- **Private tabs:** a download from a private tab is in the list only while it runs, is never
  written to Krunk's database, and its notification goes when you close the last private tab.
  The file itself stays in Downloads. Paused private downloads end with the private session.
- **Site permissions.** Sites can now ask for the camera, the microphone, your location,
  notifications, keeping their data, protected media (DRM) and, inside another site, its cookies.
  Krunk asks you, naming the site the answer is for (and the embedded site that asked, when it
  is a frame); Allow or Deny, remembered for that site unless you untick "Remember". Private
  tabs remember nothing after the last private tab closes. Android's own permission is asked
  only when a site you allowed needs it; if Android refuses, Krunk says which permission is
  missing. "Approximate location" is enough where Android allows it (Android 12 and later).
  Sound that starts by itself stays blocked and muted video stays allowed; both can be changed
  per site. Virtual-reality devices are denied unless you allow them for a site.
- **Settings › Privacy › Site permissions** lists the sites with an answer; each site has a page
  with every permission as Allow, Deny or Ask, and "Reset permissions". Changes apply the next
  time the site asks, without a restart. The shield sheet of a page leads to its site's page.
- **Site notifications** arrive as Android notifications in their own channel, named with the
  site; tapping one returns to the site's tab. At most ten per site are kept.
- **Settings › Privacy › Site data** lists the sites that hold cookies or stored data, with the
  number of cookies and the stored size, read from the engine by a small built-in extension that
  reads names and sizes only. Clearing a site asks first and removes its cookies and sign-ins,
  stored data and cached files (with its permissions if you leave the box ticked); its open tabs
  are reloaded. A site that is itself a shared hosting domain (`github.io` and the like) is
  cleared on its own name only, never with every site under it. "Clear browsing data" is
  unchanged.
- **Full screen:** a video may turn the screen to landscape while it is full screen; leaving
  full screen in any way (Back, the page, another tab or screen) brings back the bars, the
  toolbar and the orientation; the screen stays on while a page is full screen.
- **Pointer lock** is not available on Android: the engine reports a lock to the page but sends
  no mouse movement while locked. Krunk adds nothing that pretends otherwise.
- **History** pages through long lists without losing visits of the same instant and groups
  them by day off the main thread. **Screens keep their place** (scroll position, folder,
  search) when another screen was opened on top, and the page is not reloaded on return.

New Android permissions, none needed to use Krunk: notifications; camera, microphone and
location (asked only for a site you allowed); running downloads in the background.

Not in this release: downloads over several connections, a choice of the download folder,
opening a downloaded APK (Android's installer needs a permission Krunk does not ask for), the
Permissions API reporting "prompt" for a remembered location answer (the site is answered
without a prompt all the same).

Tested on the Android 16 emulator (x86_64), on the release build: the upgrade from 0.3.0, a
download behind a sign-in cookie, `blob:` and `data:` downloads, pause and resume with the file
checked byte for byte, a server that cannot continue, open and share, a private-tab download,
permission prompts allowed, denied, remembered and changed, a site notification and its tap, site
data listed and cleared, screens keeping their place, pointer lock, full screen with an
orientation lock and a rotation. Not tested on a device: real camera and microphone capture, a
real location fix, notification delivery on a phone, arm64.

## 0.3.0 — 2026-11-14

The password vault. It installs over 0.2.0 and keeps tabs, bookmarks, history and settings.

- **Password vault.** Krunk can keep the user names and passwords you sign in with, encrypted on
  the device behind one vault password that only you know (at least 8 characters). The
  encryption and the file format are those of the Windows browser: Argon2id turns the password
  into the key, XChaCha20-Poly1305 seals every login, and the names of the sites are encrypted
  too. On Android the vault key is wrapped once more by a key in the Android Keystore that cannot
  leave the device (StrongBox when the device has one, otherwise the device's secure hardware).
  In the app, wrong passwords slow the next attempt down, up to a minute.
  - What the device key protects: the saved logins. A copy of the vault file cannot be decrypted
    on another device, not even with the right password.
  - What it does not protect: the vault password against guessing. Someone who has a copy of the
    file can try passwords against it on their own computer; only Argon2id slows that down
    (64 MiB of memory and 3 passes for every guess). Choose a long vault password.
- **Passwords screen** (⋮ menu › Passwords, or Settings › Privacy › Passwords): create the vault,
  unlock it, search the logins, view one (the password stays hidden until you tap Show and hides
  itself again after 30 seconds), copy the user name or the password, add, edit and delete logins
  (delete asks first), Change password and Lock now. A copied password is marked as sensitive, so
  Android 13 and later keep it out of the clipboard preview, and Krunk takes it off the clipboard
  after 30 seconds. While Krunk is on screen it does so only if the clipboard still holds that
  password; once you have left Krunk it cannot look (Android lets only the app on screen read
  the clipboard), so it clears the clipboard whatever is on it. A keyboard that keeps a clipboard
  history may have its own copy.
- **Locking.** The vault locks:
  - when you choose Lock now;
  - after a time without vault use: 1, 5, 15, 30 or 60 minutes, 15 by default ("Lock after");
  - after you leave Krunk: at once, after 30 seconds, after 1 minute (the default) or after 5
    minutes ("Lock after leaving Krunk"). The time is there so that you can switch to another
    app, for example an authenticator app for a sign-in code, and come back without unlocking
    again. It also means the vault stays open for that long after you leave Krunk; choose
    "At once" to close that;
  - always when Krunk's process ends: a newly started Krunk has a locked vault.
- **Saving and filling in pages.**
  - After you sign in on a site, Krunk offers to save the login, or to update it when the
    password changed; a login that is already saved is not offered again. "Never for this site"
    stops the offers for one site, "Offer to save sign-ins" in the vault's settings switches them
    off altogether. Without a vault the offer leads to "Create vault" and the login is saved into
    the new vault; "Don't ask again" there switches the offers off.
  - Tap a sign-in field on a site with saved logins and Krunk shows them to choose from. Krunk
    never fills a form by itself, and the engine does not hold your passwords: it gets the one
    login you picked, at the moment you pick it.
  - A locked vault stays quiet while pages load. The vault password is asked when you tap the
    field of a site that has logins, or when you choose Save.
  - "Fill from vault" in the ⋮ menu brings the choice back on a page where you closed it.
- **Private tabs:** nothing is offered by itself and there is never an offer to save. "Fill from
  vault" in the ⋮ menu is the explicit way: after it, a tap on the sign-in field shows the saved
  logins.
- **Pages without encryption (http):** before a password is filled in or saved there, Krunk shows
  a warning that you have to confirm. A login saved on an https page is never offered to an http
  page.
- **Sign-in forms inside a frame of another site are not filled.** The login belongs to the site
  in the address bar; a form that another site embedded gets nothing. This is stricter than the
  Windows browser, which offers such a form the logins of the frame's own site.
- **HTTP sign-in dialog:** it lists the saved logins of that server ("Use …"), offers "Unlock
  vault for saved sign-ins" when the vault is locked, and Krunk offers to save after a sign-in
  that worked. Proxies get no offer.
- **Fingerprint unlock** (optional, off by default, Android 11 and later): "Unlock with
  fingerprint" in the vault's settings. The vault password always works and is not stored for
  this. Enrolling a new fingerprint on the device switches it off, and so does changing the
  vault password; switch it on again afterwards.
- **Screens of the vault are blank in screenshots, screen recordings and Recents:** the Passwords
  screen, the unlock dialog, the choice of logins and the save offer.
- **One new permission:** use biometrics (for fingerprint unlock; Android does not ask for it).

What the vault on Android does not have in this version:

- **No recovery phrase. A forgotten vault password cannot be recovered**; the create and unlock
  screens say so.
- No payment cards, no sync, no import or export of logins, no password generator, and Krunk is
  not a system-wide Autofill service: it fills in its own pages only.
- **The vault stays on this device.** It is not in Android's cloud backup and not in the transfer
  to a new device, and its file cannot be opened anywhere else, because the Keystore key does not
  leave the device. A vault file from the Windows browser does not open on Android, and one from
  Android does not open on Windows, in this version.
- **When the device's key is gone.** If Android's key store reports the key as missing or no
  longer valid (app data restored from elsewhere, key store reset), Krunk says "This vault cannot
  be opened", asks for no password and offers to delete the vault behind a typed confirmation; it
  never replaces a vault silently. If the key store merely does not answer, Krunk says so and
  asks you to try again; the vault is not treated as lost.

Tested: on the emulator only (Android 16, x86_64). **Fingerprint unlock was not run on a device:**
no fingerprint can be simulated on the test emulator, so switching it on, unlocking, a new
enrolment and the fall-back to the password are covered by unit tests with stand-ins only; the
one device check is that the switch is disabled with its reason when no fingerprint is enrolled.
**StrongBox was not run on a device either:** the emulator has none and used the other Keystore
path; the StrongBox-first choice is covered by unit tests with a stand-in. The release build was checked on the emulator, including the update from 0.2.0 (tabs, bookmark
and settings kept), every flow above, the clipboard (a copied password could be pasted at once
and was gone about 50 seconds later with Krunk left in the background), and a packet capture
while the vault was unlocked and searched (no packet).

Known limits:
- The clipboard is cleared by an alarm Android may deliver some seconds late, and later still
  while the device sleeps.
- A sign-in form inside a frame of a sibling subdomain was not tried on a device (the rule that
  refuses it is covered by unit tests and an engine setting).
- With the vault locked, the offer after signing in always reads "Save this sign-in?", also when
  the login is known: Krunk cannot tell before the vault is open. Saving then updates the
  existing login.

## 0.2.0 — 2026-11-12

Ad and tracker blocking, a quieter engine, a wider address bar and themes. It installs over 0.1.0
and 0.1.1 and keeps tabs, bookmarks, history and settings.

- **Fixed: 0.1.0 and 0.1.1 contacted Mozilla and Google in the background.** Without anyone
  asking, the engine fetched Mozilla's Remote Settings (certificate data, lists) and ran a
  connectivity check, and it asked Google's Safe Browsing service for list updates although Safe
  Browsing was set to "off". Krunk promises no background traffic, so this was a bug. 0.2.0
  switches all three off and checks it with a packet capture: after a fresh install Krunk sends
  nothing until you open a page.
  - What that costs: the engine no longer receives updates of certificate revocation data (CRLite)
    or of preloaded intermediate certificates, and there is no Safe Browsing warning page. A
    certificate revoked after this version of Krunk was built is not known to it.
  - Settings › Privacy › "Update automatically" (off by default) turns the daily filter-list
    updates and Mozilla's Remote Settings on together, from the next start. Safe Browsing and the
    connectivity check stay off in every case.
- **Ad and tracker blocking.** uBlock Origin 1.76.0 is built in, unmodified, as an engine
  extension, with the filter lists that come inside it (EasyList, EasyPrivacy, uBlock Origin's own
  lists, Peter Lowe's list, the malicious-URL list). It blocks requests and hides the leftovers on
  the page, in normal and private tabs.
  - The **shield** in the address bar shows how many requests were blocked on the page. Tap it to
    switch blocking off for that site; the choice is kept. In a private tab the sheet shows the
    count and no switch, so a private visit leaves no entry.
  - An address that is itself on a list shows Krunk's page with "Go back" and "Open once".
  - Filter lists are not updated behind your back: Settings › Privacy › "Update filter lists"
    fetches them when you ask (from the lists' own hosts), or "Update automatically" does it daily.
  - When Krunk starts, pages wait a few seconds until the blocker is ready, so the first page is
    filtered too. If it is not ready after 10 seconds the page loads and the shield says so.
  - Blocking can be switched off as a whole in Settings › Privacy.
- **Tracking parameters** (`utm_…`, `fbclid`, `gclid` and the rest of Krunk's list) are now also
  removed from links you tap inside a page and from redirects, not only from addresses you type.
  Forms that are sent (POST) are never changed.
- **Toolbar.** Back and Forward moved into the ⋮ menu, as its first row together with Home; a
  long press on either lists the pages to go to. The address bar takes the room: about two
  thirds of the toolbar on a small phone. Android's back gesture still goes back in the page's
  history. Reload and stop have a full-size touch target.
- **Themes.** Settings › Appearance › Style offers the twelve styles of the Windows browser, each
  with a small preview, and Accent colour seven colours or any other. A style sets the toolbar's
  fill (plain, gradient, tinted), the corners of the address bar and the tab cards, and how the
  current tab is marked in the switcher. Light, dark and "follow the system" work as before, and
  private tabs keep their own dark look in every style. Text and icons are checked for contrast in
  every style. Without a style nothing changes, except that the current tab's card now has a
  filled title instead of a border.
- **Pages can ask for more:** files (Android's file picker, one or several, by type; a field that
  asks for a photo can use the camera app), dates, times, weeks and months, colours, "leave this
  page?", "send the form again?" and sharing through Android's share sheet. Krunk's own dialogs
  name the site that asks; closing any of them answers the page, so none hangs.
- **Links:** mail and phone links open Android's mail and phone apps (after a tap at once,
  otherwise Krunk asks first). Selected text can be searched, and a selected address opened, from
  the menu over the selection. Settings › Tabs › "Tabs opened by links" decides whether such tabs
  come to the front.
- **Tabs:** a long press on a tab in the switcher offers Duplicate, Close other tabs and Close
  tab. "Request desktop site" now stays with the tab when Android or a crash made Krunk rebuild
  the page, and over a restart.
- **About** lists uBlock Origin and its filter lists with their licences.

Known limits: custom filter lists and protection levels come later; a file you give to a page is
copied inside Krunk's storage for the upload (removed when the tab closes); whole folders cannot
be uploaded; moving tabs in the switcher is not there yet. Each APK is about 7 MB larger.

Known issues:

- **Address bar empty although a page is shown** (seen once in testing and not again when the same
  steps were repeated, once by script and once by hand; to be looked at for 0.2.1). What was done: Settings › Privacy › "Block ads and
  trackers" switched off, back to the browser, and about four seconds later a link opened from
  another app. The page loaded, unfiltered as set, but the address bar showed "Search … or enter
  address" and no shield instead of the site. Nothing wrong was shown; the site's name was
  missing.
- **"Open once" lasts a little longer than the tab.** When you open a blocked address once, the
  blocker remembers that for up to about two minutes, in memory only. This also holds for a
  private tab: for that time after the tab is closed the same address opens in a normal tab too.
  Nothing is stored, Settings lists nothing, and a restart ends it at once.

## 0.1.1 — 2026-11-11

A fix release after a full review of the 0.1.0 code, with one changed default. It installs over
0.1.0 and keeps tabs, bookmarks, history and settings.

- **The toolbar is at the top by default.** A new install has the address bar at the top. An
  install that has already saved a setting keeps the position it stored; one that never changed
  any setting moves to the top with this update. Settings › Appearance › Toolbar position
  switches between Top and Bottom.
- **Address bar:** it shows the site you are on (the host, with its end always visible) instead
  of the start of a long address. On phones "not secure" is the red sign alone and the Home
  button appears from tablet width, so the site's name fits. The lock is shown only for the page
  that is really on screen, never for an address that is still loading. The toolbar comes back
  whenever a page loads, so a hidden address bar cannot outlive a navigation or get stuck on a
  page that does not scroll. Bottom-fixed parts of a page (cookie bars, site navigation) are no
  longer covered or left floating while the toolbar hides.
- **Pages can ask:** drop-down lists (`<select>`), alert, confirm and prompt dialogs and HTTP
  sign-in now work (0.1.0 showed none of them). Each dialog names the site that asks, and from
  the second dialog of a page on you can block the rest. Links that open a new window
  (`target=_blank`, `window.open`) open a new tab.
- **Security:**
  - Addresses handed over by other apps and links opened from a page's link menu are limited to
    web addresses and are loaded like any link on a web page.
  - A new window shows no address until it has really loaded something, and a web page cannot
    put a `krunk://` address into the address bar.
  - Krunk asks before a page-started download (file name and site), and file names are cleaned
    of characters that hide or reorder them.
  - Full screen says which site entered it and how to leave.
  - Site icons are fetched with size and time limits and checked before they are decoded.
- **Tabs and restore:** a restore ends on the page you left, with its back and forward list and
  scroll position, also when you left right after a navigation or the app was closed seconds
  after starting. A page whose process crashed, or was closed by Android to free memory, shows
  "This page stopped working" with Reload instead of a dead blank view. "Restore tabs?" keeps
  the session until you answer, also across a restart; after two failed starts in a row Krunk
  asks before restoring. Tabs survive Android closing Krunk in the background whatever the
  start-up setting. "Close all" no longer loads background tabs on the way.
- **Privacy:** private tabs stay out of the Recents thumbnail and screenshots, and one private
  tab is kept alive under memory pressure so private sign-ins are not lost. "Clear browsing
  history" also clears site icons and the recently closed list. Nothing of Krunk goes into
  Android's cloud backup any more (the direct device-to-device transfer still carries your
  data). The release build no longer writes page addresses into the system log. Krunk's own
  requests (site icons, search suggestions) use the browser's user agent, not the device's.
- **Settings:** a custom search engine and a custom new-tab address can be turned on; the
  user-agent setting is applied; "Strip tracking parameters" off survives a restart, and
  tracking parameters are also removed from links other apps hand over and from the link menu.
- **History:** visits are stored under their own page's title, and addresses that only
  redirect are not listed.
- **Smaller fixes:** a database that cannot be opened no longer stops Krunk from starting; find
  in page belongs to its tab and shows its match count also when typed fast; the address editor no longer shows a suggestion from the last
  time; mail and phone links can be copied from the link menu; a cancelled history page load no
  longer ends the list; settings dialogs scroll.
- Known limits, unchanged: downloads are handed to Android without the page's sign-in; file,
  date and colour pickers and the "leave page?" question are not there yet; tracking parameters
  are not removed from links clicked inside a page.
- Internals: page state, load policy, session start-up and prompts are pure, unit-tested code
  (75 JVM and 39 Rust tests for the Android side), session rows are written in one transaction,
  the tab list is no longer rebuilt on every engine callback, the release build refuses a debug
  signature, and the publish script checks version and package of every APK.

## 0.1.0 — 2026-11-11

The first Krunk for Android preview (`android/`, `ffi/`), sharing the Rust core with the Windows
browser (core 0.15.3). It is a separate app with its own version; see the README section "Krunk
for Android (preview)" and `docs/ANDROID_FEATURES.md`. Android 8 or later, `arm64-v8a` phones
and tablets; the APK is signed with Krunk's Android release key and installs from the website
("install unknown apps" must be allowed for the browser you download it with).

- **Engine:** GeckoView 160, pinned; `arm64-v8a` devices and the `x86_64` emulator; Android 8+.
- **Browsing:** address bar with Krunk's URL-or-search rules and DuckDuckGo by default, history,
  bookmark and engine suggestions, automatic HTTPS, HTTPS-only mode, tracking parameters removed,
  back/forward/reload/stop/home, progress, title and favicon, lock / "Not secure", Krunk's error
  page, pull-to-refresh, find in page, request desktop site, pinch zoom and text size, full
  screen, share, link long-press menu (new tab, private tab, copy, share, save image, open image).
- **Tabs:** switcher grid with thumbnails, swipe to close, close all, search; private tabs in
  their own section with their own theme and no history; recently closed; tabs survive restarts
  and process death, with a "Restore tabs" prompt after a crash; background tabs sleep and can be
  discarded (the Windows policy).
- **New tab page:** most visited sites, shortcuts (pin, edit, remove), recently closed, the ten
  built-in wallpapers or a colour, light and dark.
- **Bookmarks and history:** manager with folders and search, Netscape HTML import and export,
  history by day with search, delete and clear.
- **Settings:** search engine and custom engine, homepage, start-up, new-tab page, theme, toolbar
  at the bottom or the top, text size, force zoom, suggestions, HTTPS-only, tab sleep/discard,
  new-tab background, user agent, clear browsing data; About with versions and licences.
- **Shell:** bottom toolbar that hides while scrolling, light/dark/system, edge-to-edge, gesture
  and three-button navigation, predictive back, landscape and tablet layouts, 48 dp targets and
  content descriptions for TalkBack.
- Not in this preview: blocking, vault, downloads manager, permissions, media controls, sync,
  VPN, agent, extensions, updates, release signing (the matrix lists each with its round).
