# Krunk for Android (preview)

Krunk for Android and Android TV, for users and developers. Part of the [Krunk README](../README.md); the other pages are listed there under *Documentation*.

Krunk for Android (0.6.0) is an installable preview of the same browser for phones and
tablets (Android 8 and later, `arm64-v8a` devices and the `x86_64` emulator), and since 0.12.0
for Android TV and Google TV (with an `armeabi-v7a` APK for 32-bit TVs; see Android TV below). It is built from
this repository (`android/`, `ffi/`) and shares Krunk's Rust `core` with the Windows app: the same
address-bar rules (DuckDuckGo by default), the same SQLite database layout for history, bookmarks,
new-tab shortcuts and settings, the same session format and background-tab policy. Details:
`docs/ANDROID_PLAN.md` (decisions), `docs/ANDROID_FEATURES.md` (what is there and what is not,
feature by feature), `docs/ANDROID_DEV.md` (building, testing, signing and publishing),
`CHANGELOG-android.md` (its own release notes).

**Download.** The website (https://krunk.kruxsys.com, "Download for Android") offers the signed APK:
`Krunk-android-<version>-arm64-v8a-release.apk` for phones and tablets, plus a universal and an
`x86_64` (emulator) build, with SHA-256 checksums. Android asks once to allow installs from the
browser you downloaded it with. The APKs are signed (APK Signature Scheme v2 and v3) with Krunk's
Android release key; its certificate's SHA-256 fingerprint is
`C9:4B:89:15:CD:8B:39:3A:5F:C4:8B:C1:32:A6:DE:02:63:74:E7:63:37:C4:34:1C:59:DD:60:32:83:73:E6:24`
(check with `apksigner verify --print-certs`). Later releases are signed with the same key, so
they install over this one and keep your data.

**Updates (since 0.5.0).** Settings › About › **Check for updates** asks krunk.kruxsys.com for the
signed update information (`/updates/android-arm64/latest.json` and its Ed25519 signature, made
with the same key as the Windows updates) and, after you tap **Download and install**, fetches the
one APK for your device. Krunk installs only an APK that matches the signed size and SHA-256 and
carries the installed app's certificate; then Android's installer asks you. Krunk never checks by
itself: not at start-up, not on a timer, not in the background (`docs/NETWORK.md`, last section).
**0.4.0 and older have no update check:** install the 0.5.0 APK from the website over them once;
your data stays. A Google Play build (no update check; Play updates it) is built but not
published yet.

**Krunk account and sync (since 0.6.0).** Settings › Account & sync signs the phone in to the
Krunk account you use on Windows. As on Windows, it is a sign-in by code: the app shows a code and
opens the approval page, where you approve the phone with your account password. Then **Turn on
sync** gets the keys in one of three ways:
- **From a device that syncs:** Windows' krunk://sync shows "A device asks to join"; Approve shows
  6 digits that you type on the phone (a wrong code ends the request).
- **With the sync passphrase or recovery key:** Krunk lists the devices that keep access before
  it joins.
- **As the first device:** for an account that doesn't sync yet, the phone makes the passphrase
  and the recovery key.

Behaviour:
- **Shared code, same rules:** the keys, the merge and the conflict rules are Windows' code (the
  shared Rust core through `ffi::KrunkSync`), so the data is end-to-end encrypted exactly as on
  Windows.
- **What syncs:** bookmarks, history, settings, open tabs, new tab pins, ad-blocker exceptions and
  device names; passwords and the never-save list while the vault is unlocked. Phone-only
  settings (toolbar position, text size) stay on the phone.
- **The status** shows the address, the last sync and its result, a switch per kind of data,
  **Sync now** and the other devices' open tabs.
- **When it runs:** only while Krunk is open: a minute after start, every 5 minutes, and 30 s
  after a change.
- **No request before sign-in:** nothing goes to krunk.kruxsys.com before you tap Sign in (checked
  with a packet capture of the app's traffic).
- **Stored where:** `files/krunk-sync/account.json` (the sign-in) and `sync.json` (sync's keys), in
  the core's formats, wrapped by the key store key `krunk.sync.wrap.v1`. They are excluded from
  every backup and phone-to-phone transfer; the passwords key lives only inside the vault.
- **Removed elsewhere, or signed out:** the phone deletes its sign-in, its sync keys (also the
  vault's part) and sync's bookkeeping. Your data stays unless you also remove what came from sync.
- **Not on the phone yet:** approving or removing other devices, Finish securing, and the QR
  sign-in. Use Windows or the website for those.
- Design: `docs/sync/`; Android specifics: `docs/ANDROID_PLAN.md` §4.

**Engine.** GeckoView 160 (Firefox's engine as a library, MPL-2.0), not the system WebView and not
a Chromium build: it lets Krunk run uBlock Origin itself as a built-in extension, keeps
private tabs and site data under the app's control, and keeps the engine independent of Google's.
Pages therefore render with Gecko on Android and with Chromium on Windows.

**Ad and tracker blocking (since 0.2.0).** uBlock Origin 1.76.0 is built in, unmodified (its
signed release, pinned in `android/ubo.lock` and compared file by file at build time), with the
filter lists inside it. The shield in the address bar shows what was blocked on a page and
switches blocking off per site; private tabs are filtered too. Krunk's own small extension keeps
uBlock Origin off the network: filter lists are updated when you ask in Settings › Privacy, or
daily if you turn "Update automatically" on. Tracking parameters are removed from typed
addresses, from links you tap and from redirects.

**No background traffic, and what that costs.** After a fresh install Krunk for Android sends
nothing until you open a page (checked with a packet capture of the app's traffic). 0.1.0 and
0.1.1 did not keep that promise: the engine contacted Mozilla (Remote Settings, a connectivity
check) and Google (Safe Browsing list updates) in the background. 0.2.0 switches these off. The
price: the engine gets no updates of certificate revocation data (CRLite) or of preloaded
intermediate certificates, and there is no Safe Browsing warning page. "Update automatically"
(off by default) turns Mozilla's Remote Settings on together with the filter-list updates; Safe
Browsing and the connectivity check stay off in every case.

**What it does.** Browsing with a toolbar at the top (or, in Settings › Toolbar position, at the
bottom) that hides while you scroll: a wide address bar with the shield, the tab counter and the
menu, whose first row has Back, Forward and Home (long press for the page list);
address bar with history, bookmark and search suggestions, automatic HTTPS, Krunk's error page,
find in page, desktop-site switch (kept per tab), full screen, share, link long-press menu (new
tab, private tab, copy, share, save image), search or open selected text, mail and phone links,
page dialogs (drop-down lists, alert/confirm/prompt, HTTP sign-in, file, date, time and colour
pickers, "leave page?", "send again?", sharing), the password vault with save and fill in pages
(since 0.3.0, below) and
links that open a new tab; tabs with a switcher grid (long press: duplicate, close others), a
separate private section, recently closed,
restore after a restart or a crash, and sleeping/discarding of background tabs; the twenty styles
and the accent colour of the Windows browser (Settings › Appearance); a native new-tab
page with your most visited sites, shortcuts and the built-in wallpapers; bookmarks with folders,
search and Netscape HTML import/export; history by day; settings (search engine, homepage, start-up,
theme, toolbar position, text size, HTTPS-only, suggestions, clear browsing data) and an About page.
Light, dark and system themes; phone and tablet layouts; TalkBack-friendly controls.

**Not there yet** (see the matrix for the planned round): custom filter lists and protection
levels, for the vault a recovery phrase, a password generator, payment cards, import and export of
logins and a system-wide Autofill service (the vault fills in Krunk's own pages only),
downloads over several connections and a choice of the download folder, pointer lock (the
engine has none on Android), media controls and picture-in-picture, the AI agent, userscripts,
extensions, a Play Store listing and F-Droid. Builds made without the release key (any
debug build, or `./gradlew assembleDirectRelease -Pkrunk.allowDebugSignedRelease=true` on another
machine) are signed with a debug key and cannot be updated in place by a published release:
uninstall them first.

**Downloads, site permissions and site data (since 0.4.0).** Krunk saves downloads itself from
the answer the page received, so a file behind a sign-in is the file and `blob:`/`data:` links
work; a download a page starts asks first. ⋮ menu › Downloads lists them with open, share, pause,
resume, retry and restart; a notification shows the progress, and downloads go on in the
background. Pause and resume work where the server allows it; otherwise Krunk says so and offers
Restart. A private tab's download leaves only the file. Sites can ask for the camera, the
microphone, your location, notifications and more: Krunk asks, naming the site, and remembers
the answer per site (Settings › Privacy › Site permissions, or the shield sheet of a page, to
change it); Android's own permission is asked only when a site you allowed needs it. Settings ›
Privacy › Site data lists the sites that hold cookies or stored data and clears one site after
asking. Full screen gives the screen, the toolbar and the orientation back in every way out.

**Certificate errors.** When a site's certificate cannot be verified (self-signed, expired, for
another name, unknown issuer), Krunk shows the host, what is wrong and the certificate's SHA-256,
with **Back to safety** and, below it, **Continue anyway**. Continuing accepts that one
certificate for that host and port until Krunk is closed; tick **Remember for this site** to keep
it. A site that shows a different certificate later asks again; private tabs never keep an
exception. Sites that demand a valid certificate (HSTS) and broken connections have no Continue.

**Connection details (0.9.0).** The shield sheet has a **Connection** part under the blocker: for
https the certificate as the engine reported it (issued to, issued by, valid from/until in the
phone's date format, the SHA-256 fingerprint, long-press to copy, the names it is for, and folded
**Certificate details** with serial number, signature algorithm, public key and SHA-1); a warning
and **Forget this exception** when you accepted the certificate yourself (the page reloads and the
warning comes back); a line when parts of the page came without encryption; "Connection is not
secure" for http. The lock (or the red sign) in the address bar opens the same sheet at that
part; it opens for every web page, also where the blocker shows no shield (an IP address). For a
page the engine counts as not fully secure it gives no certificate; there **Show certificate**
reads it over a separate connection, only when tapped, and says so. Nothing of it is stored,
synced or logged.
Settings › Privacy › Certificate exceptions lists the accepted certificates and removes one at
once.

**VPN (0.10.0).** Settings › Privacy & security › VPN imports WireGuard (`.conf`) and OpenVPN
(`.ovpn`) files from a VPN provider (file picker, or "Share"/"Open with" from a file manager or a
mail app; a profile shared as text is recognised and not put into the address bar). Import
warnings (options Krunk ignores, such as scripts) are shown before saving. **Only Krunk's own
connections use it**: pages in normal and private tabs, service workers, WebSockets, downloads,
the blocker's list updates, and Krunk's own requests (sync, the update check and download,
favicons, search suggestions, the certificate check of the error panel). Other apps on the phone
are not affected: there is no Android `VpnService`, no key icon in the status bar, no VPN in the
system settings. Options: **Connect when Krunk starts**; **Everywhere** or **Private tabs only**
(then only private tabs, and Krunk's requests made for them, use the VPN; sync and updates go
direct); **Reach the local network directly** (off by default; localhost always is); **Show the
VPN in the address bar** (a mark next to the shield: green connected, amber connecting, red not
connected; tapping it opens a sheet with the profile, the tunnel address, the traffic, Connect /
Disconnect and the scope).
- **Fail-closed.** While the VPN is on but not connected (connecting, reconnecting, the server
  down, no network) Krunk loads nothing: pages show **VPN not connected** (Retry, Open VPN
  settings), Krunk's own requests fail. Nothing goes direct. An address outside what the profile
  routes shows **Not reachable through this VPN**.
- **How it works.** The same `vpn` crate as on Windows, inside `libkrunk_ffi.so`: WireGuard
  (boringtun) or OpenVPN 3 Core in user space, smoltcp as the TCP/IP stack, and a proxy on
  `127.0.0.1` (a random port). Android cannot tell which app opened a loopback connection, so each
  VPN session has a new random 32-byte credential that only Krunk's process knows. Both the engine
  and Krunk's own requests connect with SOCKS5 (RFC 1928) and that credential as the password
  (RFC 1929, user `krunk`); host names are sent to the proxy unresolved, so the phone's DNS never
  sees them. GeckoView does not send `Proxy-Authorization` to an HTTP proxy and would ask the
  user on a 407, which is why it is SOCKS5. Any other app that finds the port is disconnected
  without an answer. The
  engine is pointed at the proxy by a channel filter in Krunk's built-in `krunk-guard` extension
  (the pattern of Firefox's IP Protection), with the engine's proxy settings as a backstop that
  points at a closed port, so a connection the filter misses fails instead of going direct (the
  real port and the credential are never in the engine's settings). These settings live on the engine's default
  branch and are never saved: after a restart with the VPN off nothing of it is left. While the
  VPN is on, DNS prefetch, HTTPS DNS records, DNS-over-HTTPS, HTTP/3 and WebTransport are off,
  and WebRTC uses no local addresses and no direct UDP (calls need a TURN server reachable over
  TCP). Names are resolved by the VPN's DNS server inside the tunnel; the system resolver is used
  only for the VPN server's own name.
- **Profiles** are stored encrypted (AES-GCM, key wrapped by the Android Keystore) in
  `files/krunk-vpn/`, never synced and kept out of backup and device transfer.
- **In the background** the tunnel runs as long as Android keeps Krunk's process; there is no
  foreground service and no notification. When Android suspends or stops the app (Doze, battery
  savers, which differ per phone maker), the tunnel stops with it and reconnects when Krunk comes
  back. Fail-closed holds throughout. A change of network (Wi-Fi ↔ mobile) reconnects at once.
- **How fast "not connected" shows:** WireGuard notices a dead server after about 17 s. OpenVPN
  notices it when the server's keepalive says so (often 60–120 s); until then a page through the
  dead tunnel ends as "The site took too long" — nothing goes direct in the meantime either.
- **Not covered:** other apps; UDP through the tunnel (so no HTTP/3 and no direct WebRTC media);
  OpenVPN's interactive challenges (MFA codes). Tested on the x86_64 emulator against Krunk's own
  test servers (WireGuard, OpenVPN over UDP and TCP), not on an arm64 phone and not with a
  commercial provider.

**Password vault (since 0.3.0).** Krunk keeps the user names and passwords you sign in with in
a vault, opened with one vault password that only you know (⋮ menu › Passwords, or Settings ›
Privacy › Passwords). The encryption and the file format are those of the Windows browser
(Argon2id for the password, XChaCha20-Poly1305 for every login; the names of the sites are
encrypted too); on Android the vault key is wrapped once more by a key in the Android Keystore
that cannot leave the device (StrongBox when the device has one).
- **What a locked vault protects against:** someone who gets hold of the file (a copy, another
  app, a lost phone examined from outside). The saved logins in a copied file cannot be decrypted
  on another device, not even with the right password, because the device's key stays in the
  device.
- **What the device key does not do:** protect the vault password against guessing. The file
  contains a check record sealed by the password alone, so someone who has a copy can try
  passwords on their own computer, slowed only by Argon2id (64 MiB of memory and 3 passes for
  every guess). Choose a long vault password; a few unrelated words are easy to keep and hard to
  guess.
- **What an unlocked vault does not protect against:** someone holding your unlocked phone while
  the vault is open; malware with root access or a debugger on the app; accessibility services
  and the keyboard app you allowed, which see what you see and type; and the page itself, since a
  filled-in password is in the page, as with any password manager.
- **Where it is:** `files/krunk-vault/vault.json` in the app's private storage (next to it
  `state.json`: the wait after wrong passwords, the list that lets a locked vault know which sites
  have logins, and the fingerprint-unlock data; no password is in it). The folder is not in
  Android's cloud backup and not in the transfer to a new device, and it is bound to this
  device's key store.
- **Backing it up by hand:** in this version there is no real way. The file alone cannot be
  restored anywhere else, because the device key does not travel, and there is no export. The
  only backup today is to keep your logins somewhere else too, for example in Krunk on Windows,
  entered there by hand: a vault from Windows does not open on Android or the other way round,
  and Android has no sync yet. Uninstalling Krunk, clearing its data or resetting the phone
  removes the vault for good.
- **A forgotten vault password cannot be recovered** on Android in this version: there is no
  recovery phrase here. The create and unlock screens say so.
- **If the device's key is gone** (Android's key store reports it as missing or no longer valid,
  for example after app data was restored from elsewhere or the key store was reset), Krunk says
  "This vault cannot be opened" without asking for a password and offers to delete the vault
  behind a typed confirmation; it never puts a new vault in its place silently. A key store that
  merely does not answer is reported as such, with "try again", and is not treated as a lost key.
- **Locking.** Lock now in the Passwords screen; "Lock after" 1, 5, 15, 30 or 60 minutes without
  vault use (15 by default); and "Lock after leaving Krunk": at once, after 30 seconds, after
  1 minute (the default) or after 5 minutes. The last one exists so that you can fetch a sign-in
  code from an authenticator app and come back without unlocking again. The trade-off: the vault
  stays open for up to the chosen time after you leave Krunk, so someone who takes the unlocked
  phone in that time finds it open. Choose "At once" to close that. A Krunk that was closed by
  Android or restarted always begins with a locked vault.
- **In pages:** after a sign-in Krunk offers to save the login (or to update a changed password;
  "Never for this site"; the offers can be switched off). To fill, tap the sign-in field and
  choose a saved login. Krunk never fills by itself, and the engine gets a password only when
  you pick that login. A locked vault asks for its password at that moment, not while pages load.
  A sign-in form inside a frame of another site is not filled. The HTTP sign-in dialog offers the
  saved logins of that server and a save afterwards.
- **Private tabs:** nothing is offered by itself and nothing is ever offered for saving. "Fill
  from vault" in the ⋮ menu, then a tap on the sign-in field, shows the saved logins.
- **Pages without encryption (http):** a warning you have to confirm comes before a password is
  filled in or saved there, because anyone on the network between you and the site can read it.
  A login saved on an https page is never offered to an http page.
- **Fingerprint unlock** is optional and off by default (Android 11 and later, vault settings ›
  "Unlock with fingerprint"). The vault password always works and is still needed after a new
  fingerprint is enrolled or the vault password is changed: both switch fingerprint unlock off.
  It was **not tested on a device** for 0.3.0 (the test emulator cannot simulate a fingerprint;
  unit tests with stand-ins only), and neither was StrongBox.
- The vault's screens and dialogs are blank in screenshots, screen recordings and Recents. A
  copied password is marked sensitive and removed from the clipboard after 30 seconds: while
  Krunk is on screen only if the clipboard still holds it; once you have left Krunk the clipboard
  is cleared whatever is on it, because an app in the background cannot read it. A keyboard with
  a clipboard history may keep its own copy.

**Data and backup.** Krunk's database is `files/krunk/browser.db` in the app's private storage
(`/data/data/org.krunk.browser/files/krunk/`); the Gecko profile (cookies, site data, caches) is
in GeckoView's default location under the same `files/` directory. Since 0.1.1 nothing of Krunk
goes into Android's cloud backup: bookmarks and settings share one database file with the
browsing history and the open tabs, so it is all or nothing, and the answer is nothing.
Device-to-device transfer (the direct copy to a new phone, Android 12 and later) carries
`files/krunk`; the Gecko profile is in neither
(`android/app/src/main/res/xml/backup_rules.xml`, `data_extraction_rules.xml`). To keep bookmarks
elsewhere, use Bookmarks › Export. `adb backup` is gone in current Android; on a debug build
`adb shell run-as org.krunk.browser` reaches the files. Private tabs write nothing to the
database, and their pages are kept out of the Recents thumbnail and screenshots. The password
vault (`files/krunk-vault`) and the VPN profiles (`files/krunk-vpn`) are in neither backup nor
transfer; see "Password vault" and "VPN" above.

**Promises.** No telemetry; the app makes no network requests of its own: only the pages you
open, search suggestions while you type (on by default, off in private tabs, can be switched off)
and the favicon of a page you visited (all of them through the VPN while it is on). The Google Play Services passkey library that GeckoView
depends on is bundled (passkeys need it) and only used when a site asks for a passkey; for devices
without Google Play services, `./gradlew assembleDebug -Pkrunk.noGms=true` builds without it.

**Testing status (round 1).** Every feature above was exercised on the sandbox's Android 16
x86_64 emulator with screenshots (typed addresses and searches, suggestions, the lock and "Not
secure" indicators, the error page, find in page, the link menu, the hiding toolbar, open in new
tab, the tab switcher with close and reopen, session restore after a force-stop, private tabs
leaving no history, bookmarks, settings with the theme and toolbar-position switches, full screen,
landscape and the tablet layout). The emulator's graphics stack hung twice during the round (see
`docs/ANDROID_DEV.md`); the crash-restore prompt and Netscape import/export were covered by unit
tests only. The round report lists each flow.

**Building.** `android/scripts/setup-sdk.sh` installs the pinned toolchain (`android/toolchain.lock`),
then `cd android && ./gradlew assembleDebug` builds the Rust core for both ABIs with `cargo-ndk`,
generates the Kotlin bindings with UniFFI and packages the APKs. `docs/ANDROID_DEV.md` has the
emulator scripts, the tests and the known traps.

## Android TV (0.12.0)

Krunk for Android also runs on **Android TV and Google TV** (Android 8.0 / API 26 and later).
It is the same app and the same APK, not a separate TV build: on a TV it shows up in the
launcher's app row with its banner, and it can be used with nothing but the remote's arrows,
Center, Back and (where the remote has one) Menu. Phones and tablets are unchanged.

**Installing.** Krunk is not on the TV's Play Store yet. Sideload it:
- With the **Downloader** app (or any TV file manager), open
  `https://krunk.kruxsys.com/download/android.json` to see the current files, or go straight to the APK:
  `https://krunk.kruxsys.com/download/android/<version>/Krunk-android-<version>-<abi>-release.apk`.
  Most Google TV and Android TV devices are 64-bit (`arm64-v8a`); **32-bit TVs, such as the
  Chromecast with Google TV, need the `armeabi-v7a` APK**; the `universal` APK works on all of
  them but is larger. Allow "Install unknown apps" for Downloader when the TV asks.
- Or from a computer: `adb connect <tv-ip>` (with network debugging switched on in the TV's
  developer options), then `adb install Krunk-android-<version>-<abi>-release.apk`.
- Updates: Settings › About › **Check for updates** works on the TV as on a phone and fetches the
  APK for the TV's own ABI from the same signed manifest (`updates/android-arm64/latest.json`,
  which since 0.12.0 also lists the `armeabi-v7a` APK). If the TV has no "Install unknown apps"
  settings page, Krunk says so; install the new APK with Downloader instead.

**Remote controls.**

| Where | Key | What it does |
|---|---|---|
| Toolbar, menus, Settings, dialogs | Arrows | move the focus (a thick outline and tint on the focused element) |
| | Center | open or switch the focused element |
| | Back | close the dialog or menu, or go back one screen |
| Address bar | Center | edit the address with the TV keyboard; Enter/Go loads it |
| Toolbar | Down | into the web page |
| Web page | Up / Down | scroll |
| | Left / Right | the previous / next link or form field (like Shift+Tab / Tab) |
| | Center | open the focused link or button; on a text field, show the TV keyboard |
| | Up at the very top, or Menu | back to the address bar |
| | Back | the previous page; on the first page, the address bar |
| Cursor mode | hold Center, or menu › **Cursor mode** | a pointer appears in the middle of the page |
| | Arrows | move it (faster the longer an arrow is held); at an edge the page scrolls |
| | Center | click where the pointer is (works on anything, also elements that cannot take focus) |
| | Back, or hold Center again | leave cursor mode |

The first element (the address bar at start-up, the first entry on each screen) has the focus
whenever a screen opens. Krunk's own UI keeps inside the TV's overscan safe area (5 % at the
sides and the top); pages and full-screen video use the whole screen. The toolbar is always at
the top on a TV, whatever the toolbar-position setting says.

**User agent.** Krunk keeps its normal mobile user agent on TV: the sites tried rendered fine in
a 960 dp wide landscape window (Wikipedia, DuckDuckGo and YouTube's mobile site were tried). **Request desktop site** in the menu switches a site to the
desktop layout when it looks better that way.

**What a TV doesn't do.** Share is hidden; fingerprint unlock is not offered (there is no
fingerprint hardware); a missing file picker (VPN profile import, bookmark import/export, page
uploads), camera or settings page gets a short "This device has no app for that" message.
Downloads, notifications and the update check work as on a phone. Signing in to the Krunk account
uses the same web sign-in page with the TV keyboard.

**Known limitations.**
- The remote-control checks (`android/scripts/tv.sh tv`) ran on the KVM phone emulator switched to
  1920×1080 at 320 dpi with a test build forced into TV mode (`debug.krunk.force_tv 1`, compiled
  out of release builds), because the build machine has no KVM for an Android TV emulator image.
  The Android TV image itself (API 36, software emulation, too slow for the D-pad flows) was used
  for the launcher (Krunk's banner in Installed Apps, opened from its tile) and the start-up of the
  release APK (TV layout, the address bar focused, no crash; `tv.sh launcher`). **No real TV, no real 32-bit ARM device and no Fire TV were
  tested**; the `armeabi-v7a` APK is verified by build and `aapt2 dump badging` only.
- Fire TV (Fire OS) is not a target. The APK should install and work there when sideloaded,
  since Fire OS is Android, but it has not been tried.
- Text fields: on a hardware remote Compose moves the focus out of a text field with Up/Down; in
  the main window Krunk also does this for remotes that Compose doesn't recognise. In a dialog a
  text field may need Back (to close the keyboard) and then Down to reach the buttons.
- Up/Down scroll by Gecko's arrow-key step, which is short on long pages; cursor mode (hold an
  arrow against the edge) scrolls faster.
- A long press of Center is cursor mode, so the long-press menu of a link (open in new tab, copy
  link) is not reachable with a remote.
