# LightBrowser security notes

## Trusted UI vs. web content

### Origin split
The browser's own UI is split across **two separate origins** under the reserved `.invalid` TLD:

| Origin | Documents | Loaded in |
|--------|-----------|-----------|
| `https://shell.lightbrowser.invalid` | `/shell.html` (tab strip, toolbar, bookmarks bar, find bar) and `/popup.html` (menu, shield, bookmark folders, password prompts) | only the shell WebView and the popup WebView |
| `https://lightbrowser.invalid` | built-in pages: new tab, history, bookmarks, downloads, settings, passwords, about | tabs |

Consequences:
- **Tabs can't become the shell.** A tab can never load the shell origin; `allow_navigation`
  cancels it. Messages from tabs are classified on the tab channel, which never yields
  `Sender::Shell` or `Sender::Popup`. So even a compromised built-in page can't use shell-only
  commands such as closing or reordering tabs.
- **Separate browser storage.** Each origin has its own storage, so a built-in page can't read
  or tamper with shell state.
- **Exact matching.** The shell and popup channels accept only their own document path on the
  shell origin. Sub-frames are never trusted. A different scheme, port or subdomain, or a
  look-alike host, is rejected (`core::protocol::classify` tests).
- **Cached favicons.** `/favicon?host=` serves cached favicon bytes on both origins. Only valid
  host names are accepted, and the response is a PNG with `nosniff`.
- The browser chrome (shell), the popup and the built-in pages run on two internal origins under
  the reserved `.invalid` TLD. They are served from bytes embedded in the exe, with a strict CSP.
- The host accepts privileged messages only after `core::protocol::classify` has checked:
  - the channel (which WebView the message arrived on);
  - the exact origin and document;
  - that it came from the top-level frame.
- Each sender has a command allow-list (`Sender::permits`). For example, the new-tab page can't
  reach the vault, and the shell can't list passwords.
- Web pages can only send `ContentMessage`, which is strictly parsed and size-capped.
- Tabs can never load the shell origin. A web page can't navigate a tab to a built-in page on its
  own; only the host can (`core::assets::allow_navigation`).

## Password prompts live outside the page
- The "Save password?" and "Fill saved login?" prompts are drawn in the **popup window**. It is a
  separate top-level window owned by the browser frame, and its document is on the shell origin.
- Page content can't draw over it, style it, read it, or send it input. That rules out
  spoofing the prompt or click-jacking it with overlays or transparent iframes.
- Every prompt carries the `tab_id` and exact origin it was raised for, and the reply echoes both.
  Before filling or saving, the host re-checks that the tab is still the active tab and is still
  on that origin; otherwise the reply is dropped.
- Hiding the popup for any reason (focus loss, Escape, window move or resize, tab switch) counts
  as a dismissal, never a confirmation.

## Scripts injected into web pages
- **Which scripts:** three small scripts from `ui/inject/`, embedded and never served over any
  origin.
  - `gpc.js` and `dnt.js` only define read-only navigator properties.
  - `page-tokens.js` (top frame only) reports the page's class names and ids for generic
    cosmetic filtering.
- **What a page can do with the token message:** it can forge or flood it, but the message is
  strictly parsed (at most 2000 tokens of up to 100 characters each). Its only effect is extra
  hiding CSS on that same page, and the host applies it only if the sender's origin equals the
  tab's current top-level origin.
- **How the CSS is injected:** as a JSON string literal into a fixed script template, so it
  can't break out into code (`assets::style_injection_script`, tested).

## Filter-list updater
- It downloads only from the fixed HTTPS URLs in `core::blocker::list_url`, over WinHTTP with
  TLS 1.2 or higher, a 16 MB cap and a 30 s timeout.
- Content is validated before use. A stored download is used only if its hash matches
  `state.json`; otherwise the embedded list is used.
- It respects the configured HTTP proxy. With a SOCKS proxy it does not download at all, to
  avoid revealing the user's IP outside the proxy.
- No telemetry: the only background requests LightBrowser itself makes are these downloads, at
  most once a day.

## Password vault: design and threat model

### Assets
| Asset | Where it lives |
|-------|----------------|
| Saved logins (origin, username, password, note) | `vault.json`, encrypted. Plaintext exists only in memory while the vault is unlocked, in the UI while an entry is shown or edited, and on the clipboard for at most 30 s after "Copy". |
| Master password | Never stored. It passes once through the passwords page or the unlock prompt to the host, is wiped after the KDF, and is never logged. |
| Keys (KEK, vault key VK) | KEK is derived on unlock and dropped right away. VK is held in a `Zeroizing` buffer only while unlocked, and is stored only wrapped (KEK, then DPAPI). |
| Unlock state | The host only. The UI never decides whether the vault is unlocked. |
| Never-save list | `browser.db` (`vault_never`): origins only, no secrets. |

### Attackers considered
1. **Someone with a copy of the file:** `vault.json` from a backup, cloud sync, a stolen disk or
   another account.
2. **Malware running as the same Windows user:** it can read the user's files and call DPAPI as
   that user.
3. **A shoulder-surfer or a person at an unlocked, unattended PC.**
4. **A malicious web page or iframe:** script on a site the user visits, including third-party
   iframes, ads and look-alike phishing domains.

How each one is handled:
- **(1) Copied file:** without the master password, they get ciphertext only. On another
  account or PC, DPAPI can't unwrap VK even with the password. Usernames and origins are
  encrypted too, so the file doesn't reveal which sites the user has accounts on. Tampering is
  detected.
- **(2) Same-user malware, vault locked:** it can unwrap the DPAPI layer, but it still needs the
  master password. Each guess costs one Argon2id run (64 MiB, t=3), so offline guessing is slow
  but possible against a weak password. **Vault unlocked:** out of scope (see below).
- **(3) Shoulder-surfer or unattended PC:**
  - Passwords are masked on the passwords page and in prompts.
  - "Show" asks for the master password again, and failures share the unlock rate limit.
  - The vault auto-locks after N minutes without use (default 15), on Windows lock, on
    sleep/hibernate and on exit.
  - Copied passwords leave the clipboard after 30 s and stay out of clipboard history and the
    cloud clipboard.
  - Someone at an unlocked session within the auto-lock window can still use "Copy" or fill
    logins, as in any password manager. Lower the auto-lock time to reduce this.
- **(4) Malicious page or iframe:**
  - A page can only send the three `ContentMessage` kinds, and none of them returns secrets.
  - Iframes are never heard: messages are top-level only, and the script runs only where
    `window.top === window`.
  - Fill needs a user click in the trusted popup window, plus an exact origin match taken from
    WebView2's own view of the sender.
  - Built-in pages are on a separate `.invalid` origin that pages can't navigate to or frame.
  - Details are in the table below.

### Cryptography
```
master password --Argon2id(16-byte random salt, m=64 MiB, t=3, p=1)--> KEK (32 B, never stored)
KEK --XChaCha20-Poly1305--> key-check record     (wrong password -> rejected)
KEK --XChaCha20-Poly1305--> wrapped vault key VK (random 32 B)
    --DPAPI CryptProtectData (CurrentUser, extra entropy)--> stored in vault.json
VK  --XChaCha20-Poly1305, fresh random 24-byte nonce, AAD = entry id--> each login
VK  --XChaCha20-Poly1305--> manifest (ordered entry ids + generation)
```
- **File:** `%LOCALAPPDATA%\LightBrowser\vault.json`, separate from `browser.db`. It stores
  only the KDF parameters, the salt and ciphertexts; usernames, origins and notes are encrypted
  too.
- **Integrity:** every record is authenticated.
  - Changing the salt or the KDF parameters changes the KEK, so the key check fails.
  - Swapping, removing or reordering entries is caught by the AAD and the manifest.
  - Flipping any ciphertext byte is detected.
  - All of this is covered by tests in `core::vault`.
- **Changing the master password:** new salt, new KEK and a new VK. Every entry is re-encrypted
  and written to a temp file, synced and renamed. A failure at any step (wrapping error,
  unwritable temp file) leaves the old file byte-identical and the vault usable with the old
  password (test `failed_password_change_keeps_old_vault`).

### What it protects against
| Threat | Protection |
|--------|------------|
| Someone copies `vault.json` (backup, cloud sync, stolen disk image) | They also need the master password (Argon2id makes guessing slow) **and** the user's DPAPI keys. On another account or PC the wrapped vault key cannot be unwrapped, even with the right password. |
| Someone with the file and the user's DPAPI keys (same account) | Still needs the master password; there is no offline shortcut. |
| A malicious web page | Can't read the vault: web content can only send `ContentMessage` (`loginForm`, `submit`, `requestFill`), and none of these returns secrets. A page gets a password only after the user picked that login in the trusted prompt, and only for its own exact origin (see below). |
| Phishing look-alike domains | Fill needs an exact origin match (scheme, host and port after IDNA normalization; `vault::fill_allowed`). `https://аpple.com` (Cyrillic) never matches `https://apple.com`, nor do `http://`, subdomains or other ports. The origin compared is the one WebView2 reports for the message, never a URL the page claims. |
| Cross-origin iframes (ads, embedded widgets) | The login script runs only in the top-level document (`window.top === window`). The host only listens to top-level `WebMessageReceived` (no frame events). `PostWebMessage` reaches only the top-level document. |
| Click-jacking or spoofed prompts | "Save password?", "Fill saved login?" and "Unlock" are drawn in the browser's own popup window, outside the page. The page can't draw over them or click them. Each prompt carries its tab id and origin, and the host re-checks that the tab is still active and on that origin before saving or filling. Hiding the prompt for any reason is a dismissal. |
| Online guessing through the UI | The host (not the UI) enforces an exponential delay after failures: 1 s, 2 s, 4 s … 60 s. The failure count is persisted, so restarting doesn't reset it. Wrong master passwords on "Show password" count too. |
| Unattended unlocked vault | Auto-lock after N minutes without vault use (setting, default 15), on Windows session lock (`WTS_SESSION_LOCK`), on sleep or hibernate (`PBT_APMSUSPEND`) and on exit. Locking drops and wipes the keys and decrypted entries. |
| Passwords left on the clipboard | Copies are flagged `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0` and `CanUploadToCloudClipboard=0`. The clipboard is emptied after 30 s, but only if its sequence number is unchanged, so a newer copy by the user is never wiped. |
| Leaks via logs, database, session, errors | No password or master-password material is ever logged, stored in SQLite, or put into error strings (checked at every release: greps of all log/format calls, a schema review, the `no_plaintext_in_file` tests, and a scan of the whole data dir, WebView2 profile included, on Windows after saving a login). Parse errors never echo input. Session and history URLs lose any `user:pass@`. |

### Out of scope (what it does not protect against)
- **Malware running as the same user while the vault is unlocked.** It can read process memory
  or inject into the browser. No user-mode password manager can stop this.
- **A keylogger capturing the master password.**
- **A compromised web page that you log in to.** It sees what you type or what is filled into
  its own form, as with any browser.
- **Weak master passwords.** Argon2id only slows guessing. The setup page shows a strength hint
  and requires at least 8 characters.
- **Memory hygiene is best effort.** Secrets live in `Zeroizing` buffers and are wiped on drop,
  including the master password parsed from JSON (`protocol::Secret`) and WebView2's message
  buffer. serde_json's internal scratch buffer for escaped strings, the page's DOM, and
  clipboard memory (owned by Windows) are outside our control.
- **Rolling the whole vault file back to an older copy.** This is not detected; it would
  restore older (still encrypted) logins.
