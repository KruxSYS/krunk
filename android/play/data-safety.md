# Google Play Data safety form: draft answers (Krunk for Android 0.5.0, `play` flavour)

Draft for the operator; nothing has been submitted. Play asks about data **the app (the
developer, or any library/SDK inside it) collects or shares**. "Collected" means sent off the
device; "shared" means passed to a third party. Data a user's web browsing sends to the websites
they choose is not counted as collection by the app: Play's guidance says data collected through
a webview that users use to navigate the open web does not need to be disclosed, and a browser
is that case.

Sources for every fact below: `README.md` ("Krunk for Android", "Promises", "Data and backup",
"Password vault"), `docs/ANDROID_PLAN.md` (§0 decisions, quiet engine), `docs/ANDROID_DEV.md`
("The quiet engine", "Data location", "Vault"), `docs/ANDROID_FEATURES.md` §12,
`CHANGELOG-android.md` 0.2.0 and 0.4.0, `android/app/src/main/AndroidManifest.xml`.

## What the `play` build sends over the network, and to whom

| Traffic | When | Goes to | Developer receives it? |
|---|---|---|---|
| Pages, their resources, downloads | the user opens them | the websites | no |
| Favicon of a visited site | after the user loaded a page of that host (one per host; `<link rel=icon>` or `/favicon.ico`) | that same site | no |
| Search suggestions (the typed text) | while typing in the address bar; **on by default**, never in private tabs, can be switched off, sent without cookies | the chosen search engine (DuckDuckGo by default; Google, Bing, Brave, Startpage, Ecosia or a custom one) | no |
| Filter-list updates | only when the user taps "Update filter lists", or daily if "Update automatically" is on (off by default) | the lists' own hosts | no |
| Mozilla Remote Settings (certificate revocation data, engine lists) | only while "Update automatically" is on | Mozilla | no |
| Passkey requests (WebAuthn) | only when a site asks for a passkey | handled by Google Play services on the device (GeckoView's `play-services-fido` dependency) | no |
| Update check | **none** in the `play` flavour (brief for 0.5.0; Play updates the app) | — | — |

There is no telemetry, analytics, crash reporting, advertising SDK or account on Android (no
crash handler is set up; sync is a Windows feature). Krunk's own server (krunk.209.wf) is not
contacted by the `play` build at all (to be confirmed on the 0.5.0 build, see `CHECKLIST.md`).
A packet capture per app UID showed no traffic after a fresh install until a page was opened
(0.2.0 and later; `docs/ANDROID_FEATURES.md` §12).

## Answers, question by question

### Data collection and security

| Question | Answer | Reasoning |
|---|---|---|
| Does your app collect or share any of the required user data types? | **No** | Nothing goes to the developer. Everything that leaves the device goes to a website or service the user chose, at the user's action, as in any browser (table above). |

With "No", Play does not ask the per-data-type questions and the listing shows "No data
collected" and "No data shared with third parties". If the form version in use still shows the
security questions, answer:

| Question | Answer | Reasoning |
|---|---|---|
| Is all of the user data collected by your app encrypted in transit? | Not applicable / Yes | Nothing is collected. The app's own requests (search suggestions, filter lists, Remote Settings) are HTTPS. A favicon follows the scheme of the page the user opened; Gecko upgrades `http://` to `https://` first (HTTPS-First). |
| Do you provide a way for users to request that their data be deleted? | Not applicable (nothing is held by the developer) | There is no Android account. All data is on the device: Settings › Clear browsing data, Settings › Privacy › Site data, delete the vault, or uninstall the app. Play only requires a deletion URL for apps that let users create an account. |

### Questions answered with care (operator decision)

1. **Search suggestions.** The typed text goes from the app to the search engine while typing,
   on by default. Recommended answer: not "collected", because it goes to the search engine the
   user picked (part of using the web, user-initiated, the developer never receives it).
   Conservative alternative, if the operator prefers: declare *App activity › In-app search
   history* as **collected**, **not shared**, **processed ephemerally: no** (the developer cannot
   speak for the search engine), **optional** (users can switch suggestions off), purpose **App
   functionality**. This changes the listing to show "data collected".
2. **Location, camera, microphone, notifications.** Passed only to a website the user allowed,
   after Krunk's own prompt naming the site and Android's permission prompt. Not collected by the
   developer; covered by the browser exemption. Do not declare.
3. **Passwords (vault).** Stay on the device, encrypted (Argon2id + XChaCha20-Poly1305, key
   wrapped by the Android Keystore), excluded from cloud backup and device-to-device transfer
   (`backup_rules.xml`, `data_extraction_rules.xml`, `BackupRulesTest`). A password is handed to
   a page only when the user picks a login for that page's origin. Data processed only on the
   device does not need to be disclosed.
4. **Files and docs (downloads).** Saved to the device's Downloads folder; nothing is uploaded
   by the app. A file a user picks for a page's upload field goes to that website. Do not declare.
5. **Google Play services FIDO library.** Play requires disclosing what SDKs in the app collect.
   The library is only called when a site asks for a passkey, and the request is handled by Google
   Play services. **Not verified:** whether Google's own data-disclosure guidance for
   `play-services-fido` asks apps to declare anything. Check Google's "Data disclosure" page for
   Play services libraries before submitting; if it lists items, declare them as that page says.
6. **GeckoView and uBlock Origin.** Both are inside the app. GeckoView's background services are
   switched off by Krunk's engine config (Remote Settings only with the opt-in switch; Safe
   Browsing, connectivity and captive-portal checks, add-on blocklist and region lookup always
   off). uBlock Origin is kept off the network by Krunk's `krunk-guard` extension except for
   user-started list updates, and has no telemetry. Neither sends data to the developer.

### Other App content declarations that overlap

| Declaration | Answer |
|---|---|
| Ads | The app contains no ads. |
| Advertising ID | Not used. Confirm that the merged manifest of the 0.5.0 `play` build has no `com.google.android.gms.permission.AD_ID` (see `CHECKLIST.md`). |
| Account creation / data deletion URL | Not applicable: no account on Android. |
| Government, financial features, health | Not applicable / none. |

## Gaps the privacy policy has to close first

The form must agree with the privacy policy. https://krunk.209.wf/privacy
(`dist/server/site/privacy.html`, last updated 10 November 2026) describes the Windows browser
and the account, and does not mention Android; details in `CHECKLIST.md`, step 4.
