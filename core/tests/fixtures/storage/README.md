# Frozen storage and vault fixtures (0.9.0, S5)
Written once by Krunk 0.8.0's own code (core at a26d415, unchanged since 0.8.0) through the public
API, then frozen; the migration tests copy them to a temporary folder before opening them.
- `browser-v5-0.8.0.db`: `browser.db` at schema v5. Bookmarks: bar → Krunk, News/{Example,
  Deep/{Rust, Example again}}; other → First in other, Docs, Empty folder. History: 3 URLs, 6
  visits. Tiles: 2 pinned, 1 hidden. Allowlist example.org; never-save https://never.example;
  permission lna for http://localhost:8094; a favicon; settings (homepage https://start.example/);
  a session.
- `../vault/vault-format{1,2,3}.json`: password `fixture password`, test KDF parameters (NoWrap):
  format 1 two logins (alice@bank.example `s3cret-one`, bob@mail.example:8443 `s3cret-two`);
  format 2 plus a card (4111…1111, 12/2030); format 3 plus a recovery phrase (entropy 32 × 0x07).
