# Krunk security notes

## The engine: bundled Chromium (CEF)
- **What runs:** Chromium 156.0.8078.80 through the Chromium Embedded Framework 156.0.15,
  shipped in the download. No Microsoft Edge or WebView2 component is used.
- **Supply chain:** the CEF binary distribution is not in the repository. `scripts/fetch-cef.sh`
  downloads it into its own cache directory and checks it against two hashes before it is
  extracted: the SHA-1 from CEF's build index and the SHA-256 pinned in `cef.lock`. The exact
  `cef` crate version is pinned in `app/Cargo.toml` (`Cargo.lock` records the build).
  `SHA256SUMS` in the download covers the zip, `Krunk.exe`, `Krunk.dll` and
  `libcef.dll`. The binaries are not code-signed.
- **Sandbox:** always on.
  - `Krunk.exe` is CEF's sandbox bootstrap. The browser process and every subprocess
    run through it, with `Krunk.dll` in the same folder (the bootstrap requires DLL and
    exe to be both unsigned or both signed with one certificate).
  - Renderers run at Untrusted integrity with restricted tokens; the GPU process and storage
    service are sandboxed too (`scripts/vm-sandbox.ps1` prints them).
  - `--no-sandbox` (and other weakening switches) are rejected in the "extra flags" setting.
    CEF is started with `command_line_args_disabled`, so switches typed on the exe's command
    line are ignored as well.
- **Updates are Krunk's responsibility.** A bundled engine does not update itself. Chromium
  security fixes reach users only through a new Krunk release built on a newer CEF
  (README, "Updating the engine"). Ship such releases promptly after CEF security releases.
- **Background services are off.** Component updater, secure time, Safe Browsing updates and
  similar Chromium services are disabled by default (README, "Engine background traffic").
  Consequences:
  - The certificate revocation list (CRLSet) and the Certificate Transparency list are only those
    in the release, unless the user turns on "Download engine security components".
  - There is no Safe Browsing (CEF has none) and no SmartScreen (the WebView2 build had it as an
    option). Phishing and malware URL warnings are not provided; ad and tracker blocking is not a
    substitute.
- **CEF's password manager and autofill** are turned off on every profile (preferences
  `credentials_enable_service`, `autofill.*`), so credentials only go to Krunk's own vault.

## Trusted UI vs. web content

### Origin split
The browser's own UI is split across **two separate origins** under the reserved `.invalid` TLD:

| Origin | Documents | Loaded in |
|--------|-----------|-----------|
| `https://shell.krunk.invalid` | `/shell.html` (tab strip, toolbar, bookmarks bar, find bar) and `/popup.html` (menu, shield, bookmark folders, password prompts) | only the shell view and the popup view |
| `https://krunk.invalid` | built-in pages: new tab, history, bookmarks, downloads, settings, passwords, about | tabs |

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
- **No network.** The browser chrome (shell), the popup and the built-in pages are served by a
  CEF scheme handler from bytes embedded in the DLL, with a strict CSP; unknown paths get a 404
  from the handler. The engine's host rules make both hosts unresolvable and keep them away
  from a proxy, so even a preconnect cannot leave the machine.
- **Tabs can't reach them from web content.** Every request a tab makes to an internal origin
  must come from a built-in page of the same origin (`assets::internal_request_allowed`, checked
  in the IO-thread request handler); the shell origin is never reachable from a tab. Responses
  carry `Cross-Origin-Resource-Policy: same-origin` and `X-Frame-Options: DENY`.
- The host accepts privileged messages only after `core::protocol::classify` has checked:
  - the channel (which view the message arrived on);
  - the exact origin and document;
  - that it came from the main frame.
- Each sender has a command allow-list (`Sender::permits`). For example, the new-tab page can't
  reach the vault, and the shell can't list passwords.
- Web pages can only send `ContentMessage`, which is strictly parsed and size-capped.
- Tabs can never load the shell origin. A web page can't navigate a tab to a built-in page on its
  own; only the host can (`core::assets::allow_navigation`).

## Messages between pages and the browser
- **The bridge:** in every main frame, before page scripts run, the renderer defines
  `window.__lbHost` (frozen, non-writable, non-configurable). Its `postMessage` sends the JSON
  text to the browser process in a CEF process message; the native function behind it is kept
  in a closure, not on `window`.
- **Who sent it is decided by the browser process,** from the frame the engine reports for the
  process message: its committed URL, its main-frame flag, and whether it is still the
  browser's current main frame. The message text is never trusted for this. A page cannot
  pretend to be a built-in page or the shell.
- **Same-origin frames** can reach their parent's `__lbHost` (they are the same origin, so they
  have the same rights); cross-origin frames cannot.
- **Size cap:** messages over 8 MB are refused in the renderer.
- **Browser to page:** replies and fills are sent to the main frame only and are delivered to
  that frame's listeners.
- **Secrets in IPC:** a login submit or a fill carries a password as a string through the
  engine's IPC. Krunk wipes its own copies (`Zeroizing`), but the engine's string copies
  are not ours to wipe.

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
- **Which scripts:** small scripts from `ui/inject/` and the renderer, embedded in the DLL and
  run by the renderer when a frame's script context is created (before page scripts). They are
  never served over any origin.
  - Since 0.28.0 no Krunk script replaces a native function's `toString` or wraps native
    prototypes or constructors in the page (bot defences treat such "lies" as automation).
    DNT and GPC are the engine's own (preference / feature), not injected getters.
  - `page-tokens.js` (main frame only) reports the page's class names and ids for generic
    cosmetic filtering.
  - `login.js` (main frame only) detects login forms and fills a login the user picked.
  - A media listener (every frame, event listeners only) reports whether the frame plays media,
    so the tab is not put to sleep; whether the tab is audible comes from the engine (0.28.0).
    A page can only make its own tab stay awake.
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
- No telemetry: the only background requests Krunk itself makes are these downloads (at
  most once a day) and the update check (below). The optional AI agent (0.12.0, off by default)
  talks only to the provider the user configured (next section).

## AI agent, userscripts and macros (0.12.0)
How it works: docs/HOW-IT-WORKS.md, "AI agent, userscripts and macros". The agent is off by default. While it is
off there is no panel, no agent and no provider request. Every provider entry point checks the
setting.

**What leaves the PC, and where to.**
- **Where:** only to the provider the user set up, with the user's own key. Requests are made by
  the browser process (engine URL requests: no cookies, the proxy and DNS settings apply). Krunk
  runs no AI service and sends nothing elsewhere.
- **What:** the user's messages, the context block (the active tab's address and title, the
  window's tabs, the selection, a page outline), and the results of the tools the model calls.
  Page text, HTML and screenshots are sent only when the model asks for them.
- **History, bookmarks, downloads and cookies** are sent only when the user's message asks for
  them in words and the user clicks Allow (once per conversation).
- **Passwords (0.16.0).** A password Krunk generates for the agent is never sent: the model
  gets the number of fields that were filled. A password the user types into a message for
  the agent to enter is part of that message and goes to the provider with it.
- **The scratchpad (0.16.0)** is the model's own notes and plan. Like the rest of the
  conversation it goes to that provider and nowhere else: the plan and the list of notes (name,
  size, first line) with every request, a note's text when the model reads it back.

**Never available to the agent.**
- The password vault and its saved logins, payment cards, the recovery phrase, sync keys,
  account tokens and API keys: no tool or setting key reaches them (a core test checks this).
- The value of any password field, and a password Krunk generates for it (below): it can ask
  for one to be entered, it can never read one.
- Krunk's own pages: it can't read, script, click or screenshot them, and can't open the
  passwords, sync, site-data and storage pages at all.
- Settings outside an explicit allow-list (the vault, sync, account, AI, proxy, DNS, flags,
  updates, the download folder and search or mail templates are not on it).

**Password and card fields.**
- Their values are blanked in every result; filling, typing and editing refuse them.
- `eval_js` and the `wasm_*` tools (0.19.0) are refused while one holds a value.
- Every result is scrubbed of the page's current secret values (read again after the model's code
  ran) and of card numbers.
- Screenshots mask them.
- **Limit:** cross-origin iframes are not inspected. Only those that look like payment forms are
  covered in screenshots.

**Passwords on request (0.16.0).** The rule above stays the default: the agent can't read,
type, fill or edit a password field. What is allowed, and when:
- **One tool writes password fields: `fill_password`.** It never reads one, and it never
  writes a card field (also not a security code that is typed as a password) or any other
  field.
- **Only on the user's request.** The tool is offered to the model only for a task whose
  message asks for a password to be created or entered: a word starting with `password`,
  `passcode` or `passphrase`; `signup`, `register`, `registers`, `registering`,
  `registration`; "sign up"; "create/make … account" or "new account" (English only). "Log in"
  alone is not enough. Otherwise the tool does not exist for the model, and a call is refused.
- **Only after the user allowed the site.** Before the first use on a site in a conversation
  Krunk asks, naming the site's origin (scheme, host and port). Allow or Deny holds for that
  site until the conversation ends. The call carries the allowed origin, and nothing is entered
  unless the tab shows a page of that origin when the tool runs (http or https only, never one
  of Krunk's own pages).
- **A generated password** is made in the browser process by Krunk's generator. It is entered
  only on a form where a new password is chosen (the test of "Use a strong password"), into
  the new-password field and its confirmation, not into a current-password, one-time-code or
  read-only field.
  - The model never gets it: the tool's result has the number of fields and their selectors.
  - It is kept in memory for the conversation, for the card in that window's panel: masked
    until the user presses Show (then sent to that panel only), and copied by the browser on
    Copy (kept out of the clipboard history, cleared after 30 s).
  - It is handed to the password manager's normal save flow: sending the form saves the login
    (with Undo) when the vault is open, otherwise Krunk asks; nothing is saved from private
    tabs.
  - Every later tool result and context block of the conversation is scrubbed of it, as it
    is, HTML-escaped and URL-encoded, besides the usual scrub of the page's password fields.
- **A password the user gave** in the conversation is passed on by the model, into the field
  the selector names or the focused or only password field. The model knows it, as it knows
  the rest of the message, and so does the provider. Krunk shows only its length in the step
  log.
- **Never in a log or a step.** The value is in no `krunk.log` line, no step label, detail or
  summary and no tool result. In the browser process it is held in wiped buffers, and the
  DevTools call that carries it to the page is wiped after sending.
- **Limits.**
  - The engine's own copy of that DevTools message is not wiped by Krunk.
  - Screenshots mask password fields, but they are pictures: a page that prints the password
    as plain text would show it to a model that takes images.
  - The scrub of a generated password matches it as text in three spellings; a page that
    transforms it (splits or encodes it some other way) and shows that is not caught.
  - A prompt injection could ask the model to enter a password on another site: the user is
    asked for each site. A password the user gave is known to the model, which a page could
    steer to put it elsewhere with its normal tools: give the agent only a password you would
    send to your provider, and prefer a generated one.

**Prompt injection.**
- The system prompt marks page content and tool results as data, not instructions.
- Pages have no channel into the agent: the panel is on the shell origin, which no tab reaches.
- The agent still acts with "full control" inside these limits (the operator's choice). A
  malicious page could try to steer it, but it can only steer it to what the user's browser could
  do on that page anyway, never to the secrets above.

**Keys and logs.**
- **Keys:** stored DPAPI-protected (current user, Krunk's own entropy) in the `ai_keys` table,
  unwrapped only for one request, never synced, exported, logged or sent to a view.
- **Conversations:** in memory only.
- **The scratchpad (0.16.0):** in memory only, per conversation. It is held by the window's
  session in the browser process and shown in that window's panel; its tools are answered inside
  the session and never reach the code that touches pages, files or settings. Nothing of it is
  written to the profile, the log or session restore. "New conversation" and the panel's Clear
  empty it; it is gone when the window closes. Limits: 64 notes, 32 KiB per note, 256 KiB in
  total, 100 plan items of 300 characters.
  - Its text is model output and may repeat what a page said. In the requests it sits in a
    marked block at the end of the system prompt, quoted line by line as data, and the prompt
    says it is not instructions. The panel renders it as text, never as HTML.
- **The log:** tool names, statuses and times only: never a note's or the plan's text.
- **Crash dumps:** Krunk itself writes none (no crash reporter and no dump handler are set up).
  Windows can keep its own crash data if error reporting or local dumps are configured, and
  memory can reach the page file, as with any program.

**Userscripts and macros run code the user (or the agent, when asked) wrote.**
- Userscripts run in the page's main world, never on Krunk's own pages, and in private tabs only
  when that setting is on.
- Their GM storage calls use a hidden binding that page scripts can't reach.
- Macros act on Krunk's web pages through engine input only, never on other programs or
  Windows-wide input, and every stop releases held keys.

**WASM memory inspector (0.19.0).** `krunk://wasm`, `GM.wasm` and the agent's `wasm_*` tools read
and write the `WebAssembly.Memory` objects of one tab.
- **What it can reach** is what the page's own scripts (and `eval_js`, and any userscript) could
  already reach: the tab's memories. It gives no access to other tabs, other sites' frames,
  Krunk's own pages or anything outside the renderer.
- **The page is bound by the host** to the tab it was opened from (⋮ menu); its requests name no
  tab, and only that page kind may send them (`Sender::Page(Page::Wasm)`). Opened by address it
  is bound to nothing. The agent can't open it (`agent_forbidden`); it has the gated tools.
- **Requests are typed and checked** in `core::wasm` before anything reaches a page: a closed
  set of operations (no code, no property names), sizes within the limits. The probe checks
  addresses against the memory. An instance's exported global is named by the instance's number
  and the export's place in the list, never by its name; only mutable `i32 i64 f32 f64` globals
  are written, with a value the probe checks against the global's type.
- **Instances** are listed with what the page's own scripts see of them: export names (cut to
  200 characters, page-controlled text shown with `textContent`), kinds, and the values of
  exported globals. To learn a global's type the probe instantiates 18-byte modules of its own
  in the page's context (one imported global each, no code); they run nothing.
- **The probe** lives in the inspected page's main world as an object only the host's DevTools
  session has a handle to; it takes the built-ins it uses when it is made. A hostile page can
  still lie to it (replace `WebAssembly.Memory.prototype`'s `buffer` getter, say): what it
  returns is page-controlled data, shown as text by the inspector page (`textContent` only) and
  scrubbed and capped before the agent sees it.
- **`GM.wasm` answers** go back through a function on `window` the page can find but neither
  replace nor call usefully: it settles a promise only for the tab's nonce, which page code
  never sees. The host checks the script's `@grant` again, and a script's calls only ever reach
  its own tab.
- **The hook fallback** changes `WebAssembly` in the page and is detectable; so is the DevTools
  session. Nothing about the inspector is hidden from the page, by design.

## Addresses from other apps (0.12.1)
Windows starts Krunk with `"…\Krunk.exe" --single-argument %1` for a link or page the user opens
elsewhere. Everything after the switch is one address, so a link can't add a switch of its own.
Krunk opens only http(s) addresses, `file:` URLs and absolute file paths
(`core::default_browser::launch_url`). `javascript:`, `data:`, `krunk://` and other schemes are
refused. When Krunk is already running, the new Krunk.exe hands the addresses to it with
`WM_COPYDATA` (tagged, at most 64 KiB) and exits before it touches the profile. The running Krunk
checks every address again (any program of the user's can send that message, just as it can start
Krunk.exe with an address) and opens at most 20 tabs per hand-over. The engine's own command-line
switches stay disabled. Becoming the default browser is always the user's choice in Windows
Settings: Krunk only writes its registration under HKCU (`core::default_browser::registration`).

## Update check
How it works: docs/HOW-IT-WORKS.md, "Updates". The rules are in `core::update` (unit-tested); the app only
fetches files and runs the installer.

**Signature, not HTTPS, is what is trusted.** `latest.json` is accepted only if its exact bytes
carry a valid Ed25519 signature (`verify_strict`) by the release key built into Krunk. HTTPS
(required unless the build has the `dev-updates` feature, which release builds never have:
`build.rs` refuses an `http://` server without it) protects privacy and availability. A
compromised server, CDN or TLS certificate can therefore withhold updates, but cannot make
Krunk run its own program. (The live server, `https://krunk.kruxsys.com`, is behind Cloudflare, which
terminates TLS: it is such a CDN.)

Checks, in order:
1. The manifest is at most 16 KB and the signature 256 bytes.
2. The signature (base64 of 64 bytes) verifies over the exact manifest bytes.
3. The manifest is well-formed: semver `version`; `sha256` of 64 hex digits; `size` between 1 byte
   and 1 GiB.
4. The installer `url` resolves to the update server's own origin (same scheme, host and
   port) over HTTPS.
5. The version must be newer than the running one (no downgrades). A copy older than
   `min_supported` is only pointed at the website.
6. The download goes to a fresh folder in `%TEMP%`, capped at the manifest's `size`. Its size
   and SHA-256 must match before it is run; otherwise it is deleted.
7. It runs as the current user (the installer is per user and asks for no elevation).

**What it does not protect against:**
- **Freeze:** a server (or network attacker) can keep serving an old signed manifest, so users
  are not told about a newer release. Manifests are not time-limited. The `released` date is
  shown, and the README and website announce releases independently.
- **A stolen release key:** whoever has `update-signing.key` can push any program to every
  install that checks. The key is generated on the publishing machine
  (`dist/server/gen-signing-key.sh`: mode 600, refuses to overwrite). It is never in git, the
  image or a container: `publish-release.sh` signs on the host before copying files in. Keep an
  offline backup. Rotation means a release, signed with the old key, whose build trusts the new
  key; there is no revocation list. If the key is lost, existing installs can't be updated and
  must be reinstalled by hand.
- **Unsigned installer:** the installer and binaries carry no Authenticode signature, so Windows
  cannot vouch for them (SmartScreen warns on first manual run). The manifest signature
  replaces that only for updates installed by Krunk.
- **No Mark of the Web:** WinHTTP downloads get no `Zone.Identifier`, so SmartScreen does not
  check the downloaded installer. The signature and hash checks above are the gate instead.
- **Local attackers:** someone who can write `%TEMP%` as the same user between the hash check
  and the start of the installer can swap the file. Such an attacker can already change Krunk's
  own files in `%LOCALAPPDATA%\Programs\Krunk`, so this adds no new access.

## Krunk account: pairing, data download, deletion (0.13.0)
The full treatment is in `docs/sync/THREAT_MODEL.md` (attacker 11) and `PROTOCOL.md` §6.5 and
§11.2. In short:
- **"Add a device" by QR:** the code holds a 32-byte token in the URL fragment, so it is in no
  request log; the server stores only its HMAC. It works once, for 5 minutes, one per account.
  Claims are limited to 20 per hour per IP. The signed-in browser approves only after a re-auth
  and after checking that its 6-digit confirm code matches the phone's; it sees the claimant's
  IP and country. Every approval is mailed. A paired device gets **no sync keys**, like a link.
- **The sign-in QR in Krunk** (Settings → Account) is the server's SVG. Krunk accepts it only
  after `core::account::qr_data_url` checked it (size, an `<svg>` root, no script, links,
  styles, entities) and shows it as a `data:` image, never as markup.
- **Data download:** re-auth, 5 per hour, recorded in Activity. Metadata and ciphertext only, no
  key material; only downloaded in the browser, never mailed.
- **Full IPs on the account page:** Activity, devices and signed-in browsers show the full IPs of
  the last 90 days to the signed-in owner. Accepted: it is the owner's own data, but a stolen
  session sees it too.
- **Deletion** is immediate and irreversible: the password, the TOTP if on, Turnstile and the
  typed address; one transaction; a receipt mail. Backups keep the account for up to 14 days; a
  restore deletes it again (`account_deletions`, `deletions.log`).
- **Sync reset:** re-auth and a typed confirmation, 3 per day, mailed. It deletes the server's
  copy only; no key is exposed.
- **Rollback:** the api before 0.13.0 refuses to start against a database whose sqlx migration
  list has a version it doesn't know. 0.13.0's schema change is therefore an additive "expand
  step" outside that list (`krunk_schema_steps`), and the previous api was shown to start against
  the migrated database. From 0.13.0 on, the api ignores unknown applied migrations, so later
  migrations stay rollback-safe (PLAN §4.9).

## In-browser VPN (0.14.0)
The design is in README, "VPN (0.14.0)", and `vpn/README.md`. What matters for security:
- **Scope:** only Krunk's own traffic. There is no adapter, route, driver or service, so the
  VPN can't change what other programs do, and a bug in it can't affect the system's networking.
- **Fail-closed:** while the VPN is on, the engine's proxy and Krunk's WinHTTP requests point at
  the local proxy, which answers "VPN not connected" (503) whenever the tunnel is down and refuses
  destinations outside the profile's routes (502). Nothing is sent direct as a fallback. At start
  with "Connect when Krunk starts", the proxy is in place before the first page or job; if it
  can't be, Krunk opens no tabs. Connections opened before Connect are closed (idle, HTTP/2, QUIC) and
  Krunk's own jobs restart; an HTTP/1.1 connection in use at that moment (a WebSocket, a long
  download in the engine) keeps its old route until it ends. The proxy-related Chromium switches (`--no-proxy-server`, `--proxy-pac-url`,
  `--proxy-auto-detect`, `--force-webrtc-ip-handling-policy` …) are refused in Advanced, since
  they would override the setting.
- **Leaks closed:** names are resolved inside the tunnel, never by Windows (the custom-DNS stub
  answers nothing and the engine's secure DNS is off while the VPN runs). WebRTC is limited to
  proxied UDP, i.e. none. QUIC can't use an HTTP proxy, so the engine uses TCP.
- **Who may use the proxy:** it listens on 127.0.0.1 only, with the port held exclusively. A
  connection is accepted only when Windows' TCP table shows its other end belongs to Krunk's own
  process or an engine process started from Krunk's executable; programs that Krunk starts (an
  opened download, a mail program) and any other local program are refused. A same-user program
  that inspects Krunk's memory is out of scope, as for the vault.
- **Profiles:** DPAPI-encrypted for the Windows user (own entropy), decrypted only to connect,
  kept in zeroizing memory; never logged, synced or sent to a page. Scripts in profiles never
  run. The OpenVPN server certificate's purpose is enforced (`remote-cert-tls server` becomes
  `remote-cert-eku "TLS Web Server Authentication"`, since OpenVPN 3 Core 3.11.7 ignores the
  former); a refused certificate or password stops the VPN.
- **Supply chain:** OpenVPN 3 Core comes through the `openvpn-connect` crates (one maintainer),
  vendored and pinned (`vendor/README.md`: checksums, patches, how to diff against upstream);
  its TLS and crypto run on rustls and RustCrypto, not OpenSSL.
- **Local network (0.14.2, opt-in):** with "Reach the local network directly", private
  addresses and local names go out directly, not through the VPN; everything else stays
  fail-closed. A public site can't use it to reach the internet directly, only names and
  addresses on those local lists.
- **Not covered:** traffic of other programs (by design); a VPN provider who logs; WebRTC calls
  need a TCP relay; long-lived WebRTC connections opened before Connect are not migrated.

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
    the engine's own view of the sending frame.
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
- **File:** `%LOCALAPPDATA%\Krunk\vault.json`, separate from `browser.db`. It stores
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
| Phishing look-alike domains | Fill needs an exact origin match (scheme, host and port after IDNA normalization; `vault::fill_allowed`). `https://аpple.com` (Cyrillic) never matches `https://apple.com`, nor do `http://`, subdomains or other ports. The origin compared is the one the engine reports for the sending frame, never a URL the page claims. |
| Cross-origin iframes (ads, embedded widgets) | The login script and the message bridge run only in the main frame. The host treats messages from any other frame as unprivileged and drops content messages that do not come from the current main frame. Fills are sent to the main frame only. |
| Click-jacking or spoofed prompts | "Save password?", "Fill saved login?" and "Unlock" are drawn in the browser's own popup window, outside the page. The page can't draw over them or click them. Each prompt carries its tab id and origin, and the host re-checks that the tab is still active and on that origin before saving or filling. Hiding the prompt for any reason is a dismissal. |
| Online guessing through the UI | The host (not the UI) enforces an exponential delay after failures: 1 s, 2 s, 4 s … 60 s. The failure count is persisted, so restarting doesn't reset it. Wrong master passwords on "Show password" count too. |
| Unattended unlocked vault | Auto-lock after N minutes without vault use (setting, default 15), on Windows session lock (`WTS_SESSION_LOCK`), on sleep or hibernate (`PBT_APMSUSPEND`) and on exit. Locking drops and wipes the keys and decrypted entries. |
| Passwords left on the clipboard | Copies are flagged `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0` and `CanUploadToCloudClipboard=0`. The clipboard is emptied after 30 s, but only if its sequence number is unchanged, so a newer copy by the user is never wiped. |
| Leaks via logs, database, session, errors | No password or master-password material is ever logged, stored in SQLite, or put into error strings (checked at every release: greps of all log/format calls, a schema review, the `no_plaintext_in_file` tests, and a scan of the whole data dir, engine profile included, on Windows after saving a login). Parse errors never echo input. Session and history URLs lose any `user:pass@`. |

### Out of scope (what it does not protect against)
- **Malware running as the same user while the vault is unlocked.** It can read process memory
  or inject into the browser. No user-mode password manager can stop this.
- **A keylogger capturing the master password.**
- **A compromised web page that you log in to.** It sees what you type or what is filled into
  its own form, as with any browser.
- **Weak master passwords.** Argon2id only slows guessing. The setup page shows a strength hint
  and requires at least 8 characters.
- **Memory hygiene is best effort.** Secrets live in `Zeroizing` buffers and are wiped on drop,
  including the master password parsed from JSON (`protocol::Secret`) and our copy of each page
  message. The engine's IPC strings, serde_json's internal scratch buffer for escaped strings,
  the page's DOM, and clipboard memory (owned by Windows) are outside our control.
- **Rolling the whole vault file back to an older copy.** This is not detected; it would
  restore older (still encrypted) logins.
