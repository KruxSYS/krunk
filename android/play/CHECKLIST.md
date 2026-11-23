# Google Play: the operator's checklist for Krunk for Android 0.5.0

Nothing has been uploaded or created in Play Console, and no upload key exists. The drafts next
to this file: `listing.md` (store listing), `data-safety.md` (Data safety form),
`content-rating.md` (IARC questionnaire and target audience). Work through the steps in order;
step 1 must be decided before anything is uploaded, because the first upload fixes the app's
signing certificate on Play for good.

## 0. Check the build before using it

The AAB to upload: `src/dist/play/Krunk-android-0.5.0-play-release.aab` (`play` flavour, per-ABI
splits for `arm64-v8a` and `x86_64`, minSdk 26, targetSdk 35, applicationId
`org.krunk.browser`).

- [ ] Permissions of the `play` build (`bundletool dump manifest --bundle <aab>`): expected
      INTERNET, ACCESS_NETWORK_STATE, VIBRATE, WAKE_LOCK (from GeckoView), USE_BIOMETRIC,
      FOREGROUND_SERVICE, FOREGROUND_SERVICE_DATA_SYNC, POST_NOTIFICATIONS, CAMERA, RECORD_AUDIO,
      MODIFY_AUDIO_SETTINGS, ACCESS_COARSE_LOCATION, ACCESS_FINE_LOCATION, plus
      AndroidX's `org.krunk.browser.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION`.
      **Not** present: `REQUEST_INSTALL_PACKAGES` (direct flavour only) and
      `com.google.android.gms.permission.AD_ID`. Anything else: find where it comes from before
      upload; each extra sensitive permission needs its own declaration.
- [ ] The `play` build contacts nothing of Krunk's own (no update check, no krunk.kruxsys.com). A
      packet capture of a fresh start, as in the release routine (`a3.sh gate idle10`), confirms it.
- [ ] **Target API level.** Play raises the required targetSdk every year (API 35 for new apps and
      updates since 31 August 2025). **Not verified:** whether API 36 has been required since
      31 August 2026. Check Play Console's current requirement first. If it is 36, the AAB with
      targetSdk 35 will be refused, and raising it brings Android 16 behaviour (on screens of
      600 dp and wider, apps can no longer fix their orientation or refuse resizing, which
      touches the full-screen orientation lock of 0.4.0). That is a build change with its own tests.
- [ ] 16 KB memory pages (required for apps targeting Android 15+): verified on the 0.4.0
      `arm64-v8a` APK: every native library has 0x4000 LOAD alignment and `zipalign -c -P 16`
      passes. Repeat on the 0.5.0 AAB's APKs (`bundletool build-apks`, then the same check).
- [ ] The text in `listing.md` still matches 0.5.0 (`CHANGELOG-android.md` 0.5.0 section).

## 1. Decide the app signing key (operator decision, blocks every upload)

Play re-signs every APK it delivers with the **app signing key**. The developer signs the AAB
with an **upload key**. The website APKs are signed with Krunk's Android release key
(`~/.config/krunk/android-release.jks`, `CN=Krunk, O=Krunk, C=DE`, RSA 4096), certificate SHA-256
`c94b8915cd8b393a5fc48bc132a6de026374e76337c4341c59dd60328373e624`. Android installs an update
only when it is signed with the same certificate as the installed app.

**Option 1: the existing release key becomes Play's app signing key.**
- In "Create app" › App integrity › choose "Use a different key" / "Export and upload a key from
  Java keystore", download Google's PEPK tool and the encryption public key it shows, run PEPK
  on `android-release.jks` (alias `krunk`), upload the encrypted zip. The keystore itself never
  leaves the machine; Google receives an encrypted copy of the private key.
- Upload key: either the same key (simplest; the existing release signing config signs the AAB)
  or a new, separate upload key (Google's recommendation; can be reset through Play support if
  lost, unlike the app signing key).
- Result: one certificate everywhere. Website-APK users (0.1.0–0.4.0 are already out) can move
  to the Play version and back without uninstalling, keeping tabs, bookmarks and the password
  vault. Both flavours must then keep one `versionCode` sequence (Android refuses a lower one).
  Which store updates an app that was switched over is up to Android and Play (installer of
  record); not verified, check it on the internal track (step 7).
- Cost: Google holds a copy of the key that also signs the website APKs; it cannot be taken back.
  The key's custody note in `docs/ANDROID_DEV.md` should then say so.

**Option 2: a new upload key; Play generates and keeps its own app signing key.**
- Create an upload keystore (outside the repository, backed up like the release key), configure
  the `play` flavour to sign the AAB with it, accept Play's generated key at app creation.
- Result: the Play install and the website APK have **different certificates and cannot update
  each other**. A user who switches must uninstall first and loses all local data, **including
  the password vault, which has no export and is not in any backup** in this version
  (`README.md`, "Password vault"). Bookmarks can be carried by Bookmarks › Export.
- Cost: the private release key stays only with the operator.

- [ ] Decision recorded (Option 1 / Option 2): ______
- [ ] If Option 2: upload key created and backed up; its certificate fingerprint recorded in
      `docs/ANDROID_DEV.md`. If Option 1: PEPK export done; Play shows the app signing
      certificate SHA-256 `C9:4B:89:15:…:73:E6:24` under App integrity.

## 2. Developer account

- [ ] The Play developer account exists and its identity is verified. **Personal accounts
      created after 13 November 2023** must run a **closed test with at least 12 testers for 14
      days** before they may apply for production; organisation accounts (D-U-N-S number) are
      exempt. Plan for that time.
- [ ] Developer contact email (public): **TODO**. Developer website: https://krunk.kruxsys.com.
- [ ] Note for the direct APK: Google's Android developer verification (registering package names
      and signing keys for apps installed outside Play) was announced for 2026–2027. **Not
      verified** for this project's countries; check whether `org.krunk.browser` and the
      website certificate need registering, and which certificate(s) under each option above.

## 3. Create the app

- [ ] Play Console › Create app: name "Krunk Browser", default language (see `listing.md`), App,
      Free, declarations (Developer Program Policies, US export laws).
- [ ] The package name `org.krunk.browser` is fixed by the first upload; check that Play accepts
      it (it must not be in use by another Play app).

## 4. Privacy policy (must be fixed before review)

https://krunk.kruxsys.com/privacy exists (`dist/server/site/privacy.html`, served by the Caddy rule
`handle /privacy`; last updated 10 November 2026). Gaps for Play:
- [ ] It does not mention Android at all. It says the browser "contacts this site once a day to
      check for an update" and describes the agent "from your PC": true for Windows, wrong for
      the `play` build (no update check).
- [ ] Add an Android section matching `data-safety.md`: no telemetry, no account, no update
      check in the Play version; the app's own requests (search suggestions to the chosen
      engine, on by default, off in private tabs; favicons of visited sites; filter-list updates
      on request or daily with "Update automatically"; Mozilla Remote Settings only with that
      switch); permissions passed to websites only when the user allows; the vault and all data
      stay on the device; how to delete (clear data / uninstall); passkeys handled by Google
      Play services.
- [ ] The website APK's update check (direct flavour, 0.5.0) needs a line too (what it sends).
- [ ] "Contact" names `privacy@209.wf`: decide whether that is also the Play contact address.
- [ ] Publish the site change before submitting (Cloudflare caches assets 4 h; the page itself
      should be checked live).

## 5. Store listing (`listing.md`)

- [ ] App name, short and full description, category, contact details from `listing.md`.
- [ ] **App icon 512 × 512**, 32-bit PNG, at most 1 MB: **does not exist yet.** Sources: the
      vector `assets/brand/krunk.svg` and the adaptive launcher icon
      (`android/app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml`). Render the SVG at 512 px;
      Play applies its own mask and shadow, so use the full square artwork.
- [ ] **Feature graphic 1024 × 500**, JPEG or 24-bit PNG without alpha: **does not exist.**
- [ ] **Phone screenshots**, 2 to 8: **none usable yet.** The existing emulator captures
      (`.pipeline/screenshots/android/`, 1080 × 2400) are test runs from 0.1.x with test pages
      and the old toolbar, and 1080 × 2400 breaks Play's rule that the long side may be at most
      twice the short side (2400/1080 = 2.22). Take new ones on the 0.5.0 build at a 9:16 or
      9:18 size (for example 1080 × 1920 or 1080 × 2160), showing: a page with the shield count,
      the shield sheet, the tab switcher, private tabs, settings › privacy. The vault's
      screens come out blank in screenshots by design (`FLAG_SECURE`), so they cannot be shown. No third-party logos or ads in them.
- [ ] Tablet screenshots (7" and 10"): optional; needed for tablet visibility. The tablet layout
      exists (sw600dp) but was only checked by resizing the phone emulator.
- [ ] Video: optional, none.

## 6. App content (Policy › App content)

- [ ] **Privacy policy:** https://krunk.kruxsys.com/privacy (after step 4).
- [ ] **App access:** all functionality is available without special access (no sign-in).
- [ ] **Ads:** No, the app contains no ads.
- [ ] **Content rating:** IARC questionnaire per `content-rating.md`.
- [ ] **Target audience:** 18 and over (13+ at the lowest), not appealing to children
      (`content-rating.md`).
- [ ] **Data safety:** per `data-safety.md` ("No data collected or shared"; two decisions
      there: search suggestions, and the Play services FIDO disclosure).
- [ ] **Advertising ID:** No (after the manifest check in step 0).
- [ ] **News / Government / Financial features / Health:** not applicable.
- [ ] **Foreground service permissions** (required for targetSdk 34+): declare
      `FOREGROUND_SERVICE_DATA_SYNC` for `DownloadService` (`android:foregroundServiceType=
      "dataSync"`): "transfers a file the user chose to download, while the transfer runs; the
      notification shows progress and has pause/cancel". Play asks for a short video of the
      user-visible flow (start a download, leave the app, the notification); record it on the
      0.5.0 build. Risk: Play prefers user-initiated data transfer jobs for user downloads;
      `dataSync` with Android's six-hour daily limit is what 0.4.0 ships (`CHANGELOG-android.md`).
- [ ] **Sensitive permissions:** no Play declaration form exists for foreground location,
      camera or microphone (background location would need one; Krunk does not request it).
      State in the listing or privacy policy that camera, microphone and location are used only
      for websites the user allows, after Krunk's prompt naming the site and Android's own
      prompt (`AndroidManifest.xml` comments, `CHANGELOG-android.md` 0.4.0). POST_NOTIFICATIONS
      and USE_BIOMETRIC need no declaration. If `REQUEST_INSTALL_PACKAGES` shows up in the AAB,
      stop: it belongs to the direct flavour and Play restricts it.

## 7. Release: internal testing first

- [ ] Testing › Internal testing: create a release, upload
      `src/dist/play/Krunk-android-0.5.0-play-release.aab`, add testers (email list), release
      notes from `listing.md` / `CHANGELOG-android.md`.
- [ ] Install from the internal track on a real `arm64-v8a` phone: first start (profile creation
      takes 10–20 s), blocking ready (shield count), a download in the background, a permission
      prompt, the vault, no update check in the menu/settings. Check Play's pre-launch report.
- [ ] Under Option 1 only: install the website 0.4.0 APK first, then update from the internal
      track; data and vault must survive. Under Option 2 this must fail (different certificate).
- [ ] Closed testing (12 testers, 14 days) if the account type needs it (step 2), then
      production with a staged rollout.

## 8. Review risks

- **Ad blocking.** Play allows blocking inside the app's own browsing; it forbids interfering
  with other apps or their ads (Device and Network Abuse). The listing says "in the pages you
  open in Krunk" and names no target site. A reviewer may still query it; the answer is that
  uBlock Origin runs as a WebExtension inside Krunk's own engine and touches no other app.
- **Third-party names.** uBlock Origin (author and licence named), GeckoView/Mozilla, DuckDuckGo,
  Google and the other engines appear in the description factually. Remove a name if a
  reviewer objects to it as metadata misuse.
- **Size.** The release `arm64-v8a` APK of 0.4.0 is 103.6 MB (GeckoView's `libxul.so` is most of
  it); per-ABI splits keep the Play download near that, under Play's 200 MB compressed download
  limit for an AAB. The universal APK (186 MB) is not involved.
- **Background traffic claims.** "Sends nothing until you open a page or type" is backed by the
  per-UID packet captures of 0.2.0–0.4.0; repeat it for the `play` build (step 0).
- **Play services FIDO** (passkeys) is bundled by decision (`docs/ANDROID_PLAN.md` §0); it is a
  Google library in a browser that otherwise avoids Google contact. Not a policy problem, but the
  Data safety answer depends on Google's disclosure guidance for it (`data-safety.md`, item 5).

## 9. Operator notes (not for the listing)

- **Peter Lowe's list** inside uBlock Origin is under the McRae General Public License, and its
  author asks for non-commercial use (`android/THIRD-PARTY-NOTICES.md`). Krunk is free and has
  no ads; if it is ever sold or monetised, revisit this list.
- **GPL-3.0 (uBlock Origin):** the app bundles it unmodified; the licence text is in the package
  and the source is the upstream release tag named in `android/ubo.lock`. Play distribution
  changes nothing here, but keep the About screen and `THIRD-PARTY-NOTICES.md` in the release.
- **Updates on Play:** every GeckoView security release (every 4 weeks) has to reach Play too;
  a Play review can take days. Plan the bump cadence with that delay.
- **Vault on a switch between channels:** under Option 2 a user who moves between the website
  APK and Play loses the vault. Say so on the website's Android download box if Option 2 is chosen.
