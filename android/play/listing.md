# Google Play store listing: draft (Krunk for Android 0.5.0, `play` flavour)

Draft only; nothing has been uploaded. Character limits are Play's (name 30, short description
80, full description 4000). Spelling follows the project's docs (British); set the listing's
default language to match (en-GB) or change the spelling for en-US.

## App details

| Field | Value |
|---|---|
| App name (≤30) | `Krunk Browser` (13 characters) |
| Package | `org.krunk.browser` |
| Default language | English (en-GB), see above |
| App or game | App |
| Free or paid | Free |
| Category | Communication (where Play lists web browsers; confirm in the Console) |
| Tags | Web browser (pick from Play's tag list) |
| Contact email | **TODO (operator).** Must be a real, monitored address; it is shown publicly. The website's privacy page names `privacy@kruxsys.com` for privacy questions; use it only if that is the intended public contact. |
| Website | https://krunk.kruxsys.com |
| Privacy policy | https://krunk.kruxsys.com/privacy (exists; does not cover Android yet, see `CHECKLIST.md`) |
| Contains ads | No |

## Short description (≤80)

```
A private browser with built-in ad and tracker blocking and no telemetry.
```
(73 characters)

## Full description (≤4000)

```
Krunk is a web browser that keeps out of your business. It blocks ads and trackers in the pages you open in Krunk, sends no telemetry, and makes no network requests of its own in the background.

AD AND TRACKER BLOCKING
• Krunk includes uBlock Origin, the open-source content blocker by Raymond Hill and contributors (GPL-3.0). It is built in unmodified, as a separate program, with the filter lists that come with it.
• It filters the pages you open in Krunk, in normal and private tabs, from the first page on.
• The shield in the address bar shows how many requests were blocked on a page. Tap it to switch blocking off for one site.
• Filter lists are updated when you ask (Settings › Privacy), or daily if you turn on "Update automatically".
• Tracking parameters such as utm_ and fbclid are removed from addresses you type, links you tap and redirects.

QUIET BY DESIGN
• No telemetry, no analytics, no ads, no account.
• After installing, Krunk sends nothing until you open a page or type in the address bar. Its own requests are the filter-list updates you start, the icon of a site you visited, and search suggestions while you type (switch them off in Settings; never in private tabs).
• The engine's own background services (connectivity checks, Safe Browsing list updates, Mozilla's Remote Settings) are switched off. The price is stated plainly: there is no Safe Browsing warning page, and the engine gets no certificate-revocation updates unless you turn on "Update automatically".

PASSWORD VAULT
• Save and fill sign-ins in pages, protected by a vault password only you know (Argon2id and XChaCha20-Poly1305), with the key wrapped by the Android Keystore.
• Optional fingerprint unlock (Android 11 and later).
• Krunk never fills by itself: you tap the field and pick the login.
• The vault stays on your phone. It is not in Android's cloud backup or device transfer. A forgotten vault password cannot be recovered in this version.

PRIVATE TABS
• A separate set of tabs that leaves no history, keeps its cookies and site data in memory, and keeps its pages out of the Recents screen and screenshots.

EVERYDAY BROWSING
• Address bar with suggestions from your history and bookmarks; DuckDuckGo by default, or Google, Bing, Brave, Startpage, Ecosia or your own search address.
• Automatic HTTPS, with an HTTPS-only mode.
• Tabs with a switcher grid, recently closed tabs and restore after a restart.
• Bookmarks with folders, and import and export as HTML.
• Downloads that keep your sign-in, with pause and resume where the server allows it.
• Site permissions: a site gets the camera, microphone, location or notifications only after you allow it, and you can change each answer per site.
• Site data: see which sites store cookies or data and clear one site.
• Find in page, desktop site per tab, full screen, sharing, toolbar at the top or bottom, light and dark themes, styles and accent colours, phone and tablet layouts.

ENGINE
Krunk uses GeckoView, Mozilla's browser engine for Android, so pages are rendered by Gecko.

OPEN SOURCE
Krunk is MIT-licensed. Settings › About lists every component with its licence.

Not there yet: sync, extensions, a password generator, a recovery phrase for the vault, picture-in-picture.
```

Length check: the text inside the block is about 3,250 characters (re-count after any edit;
Play counts characters, not bytes).

## Wording rules followed (and why)

- "Blocks ads and trackers **in the pages you open in Krunk**": Play's Device and Network Abuse
  policy forbids apps that interfere with other apps or block ads in other apps; in-browser
  blocking is common on Play, but the scope must be unambiguous.
- No site or brand names as targets (no "works on YouTube"), no comparison with or criticism of
  other browsers, no "best/#1", no keyword list.
- uBlock Origin is named once, with author and licence, as a bundled separate program
  (`android/THIRD-PARTY-NOTICES.md`). Mozilla/GeckoView is named only factually.
- Every claim is from `README.md` (Android section), `CHANGELOG-android.md` and
  `docs/ANDROID_FEATURES.md` as of 0.4.0. Re-check against 0.5.0's changelog before submitting
  (for example if 0.5.0 adds or changes features).
- Not in the listing on purpose: Peter Lowe's non-commercial note (operator note in
  `CHECKLIST.md`), passkeys (depend on Google Play services, not tested), anything from the
  Windows version (sync, VPN, AI agent).

## "What's new" text for the first Play release (≤500)

```
First release on Google Play.
```
TODO: replace with the 0.5.0 highlights from `CHANGELOG-android.md` once that section exists.
