# Krunk account service changelog

Changes to the account and sync service (`server/`, the site's account pages). The app's
release notes are in the repository's `CHANGELOG.md` and `CHANGELOG-android.md`.

## 2026-11-17: signed in to Krunk = signed in to the website

- Opening the Krunk website in a Krunk browser that is signed in to your account signs the
  website in as well (needs Krunk 0.24.0 or Krunk for Android 0.11.0). Such a session shows
  "Signed in by <device>" in the list of signed-in browsers. Changing the password, two-step
  verification, removing devices and deleting the account still ask for your password.
- Signing out on the website keeps it signed out until Krunk restarts.
- New endpoints `device/web-sign-in` and `web/device-sign-in`; migration
  `0011_device_web_sign_in.sql` adds a table and a nullable column and widens one check.

## 2026-11-17: one-step registration

- **Register, click the mail link, signed in.** Confirming the address now signs you in. In the
  browser you registered in no password is asked; elsewhere it is asked once. The "check your
  email" page also signs in by itself as soon as the address is confirmed.
- **Krunk signs in by itself.** A registration started from Krunk's **Sign in** (the `/link`
  page) links that Krunk to the new account when the address is confirmed. It does so only when
  the request comes from the same network address as Krunk's own request, so a sign-in link sent
  to someone else can't be used this way. If the code ran out meanwhile, the page says to click
  **Sign in** in Krunk once more.
- **No second password right after signing in.** Approving a device within 10 minutes of signing
  in no longer asks for the password again (the code check stays).
- The sign-in and registration pages keep where you came from when you switch between them.
- New endpoint `web/register/poll`, new cookie `__Host-krunk_reg`, `web/verify-email` takes the
  password as optional (401 `password_required`). Database migration `0010_registrations.sql` is
  additive: existing accounts, sessions and devices are untouched.
- The confirmation mail now says "Click to confirm your address and sign in."
