# Krunk sync: open questions for the operator

These need a decision or an input that only the operator can give. Each one has a proposed
default, so implementation can start on S1 (server skeleton) without blocking. Items marked
**blocking for Sx** must be answered before that milestone goes live.

| # | Question | Proposed default | Blocking for |
|---|---|---|---|
| Q1 | **Turnstile keys.** Please create a Turnstile widget for `krunk.209.wf` (managed mode) and provide the **site key** and **secret key**. Should the final domain be added to the same widget? | Managed widget, hostname `krunk.209.wf`, plus the final domain later. Staging uses Cloudflare's public test keys. | S2 live |
| Q2 | **SMTP provider and credentials.** Which provider (for example Postmark, Mailgun, Amazon SES, Fastmail or your own server) and which host, port, user and password? | External provider, STARTTLS on 587. Staging uses a local mail sink. | S2 live |
| Q3 | **"From" address and DNS.** Which sender address (e.g. `Krunk <no-reply@krunk.209.wf>`)? Can SPF, DKIM and DMARC records be set for that domain? | `no-reply@` the site domain, DMARC `p=quarantine`. | S2 live |
| Q4 | **Database storage and backups.** May the machine running the ai-pipe stack hold the Postgres volume (`krunk-db`)? Where should the encrypted daily backups go (path or off-site target), who holds the `age` key, and how long are they kept? | The DB volume on the stack's host. Daily `pg_dump`, `age`-encrypted to the operator's key, kept 14 days at a location the operator names (off-site preferred). | S1 live |
| Q5 | **Quotas per account.** | 50 MiB of ciphertext, 100,000 records, 20,000 history URLs (server hard cap 25,000), 64 KiB per record, 500 tabs per device, 10 active devices. | S7 live |
| Q6 | **IP retention.** | 90 days, then IPs are nulled. The country (from `CF-IPCountry`) is kept while the session or device exists. | S3 live |
| Q7 | **Offer a "no passphrase" mode** (device approval only, plus a mandatory recovery key)? Without a passphrase, restoring onto a fresh install needs the recovery key or another device. | **Not in v1.** A passphrase (generated) plus a recovery key is required. It can be added later without a format change (RR is then wrapped under the recovery key only). | S6 |
| Q8 | **How traffic reaches Caddy:** directly from Cloudflare to the host port, through a Cloudflare Tunnel, or through ai-pipe's own proxy? This decides which addresses Caddy trusts to supply `CF-Connecting-IP`. | Trust only Cloudflare's published ranges, plus the single ai-pipe proxy address if there is one (that proxy must forward `CF-Connecting-IP` unchanged). | S1 live (rate limits depend on it) |
| Q9 | **Inactivity policy.** Delete accounts unused for N days? | Off. If wanted: 730 days, with warnings at 30 and 7 days. | S10 |
| Q10 | **Deletion and backups.** Deletion is immediate in the live database, but data stays in encrypted backups until they expire (14 days), and a restore re-applies deletions. Is that acceptable for the privacy notice? | Yes, stated in the privacy notice. | S10 |
| Q11 | **Email wording and language.** English only? Who reviews the templates (new device, password change, reuse alert, escrow fetch, revocation, deletion receipt)? | English, short plain-text templates in the repo, reviewed by the operator. | S2 |
| Q12 | **Breached-password check** on registration and password change via HIBP k-anonymity (`api.pwnedpasswords.com`, sending only the first 5 SHA-1 hex characters)? | No by default. Only the bundled top-100k list is checked, so no third-party call is made. | S2 |
| Q13 | **Escrow hardening with an OPRF (RFC 9497),** so a DB thief can't even attempt offline passphrase guesses without the server's key. | Later phase (S11+). The generated passphrase makes it optional. | – |
| Q14 | **Final domain.** When `krunk.209.wf` is replaced, the API base moves too. | One release from the old domain switches clients to the new one, with the old domain redirecting for 6 months. | – |
| Q15 | **Privacy policy and legal.** Who is the data controller and what is the contact? Is a privacy page needed on the website (GDPR: email, IPs, account data)? | A privacy page in S10, using the operator's contact details. | S2 live |
| Q16 | **Cloudflare rules.** Should a WAF or rate-limiting rule be added for `/api/*` at Cloudflare as a second layer? Bot Fight Mode would break the native client's API calls, so it should be off for `/api/*`. | Cloudflare rate limit on `/api/v1/web/*` (60/min/IP); `/api/v1/*` excluded from bot challenges. | S2 live |
| Q17 | **Device limit and approval rate.** | 10 devices per account, 5 approvals per day, 10 link grants per hour per IP. | S3 |
| Q18 | **Android timeline.** Should the protocol's Android-specific details (Keystore wrapping, WorkManager) be validated by an early spike before S6 freezes the format? | A spike in parallel with S4 (crypto vectors on an emulator). | S4 |
