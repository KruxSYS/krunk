# Server data

`common-passwords.txt`: passwords refused at registration and password change
(docs/sync/PLAN.md §4.2, OPEN_QUESTIONS Q12). Derived from SecLists
`Passwords/Common-Credentials/100k-most-used-passwords-NCSC.txt` (MIT License, © 2018 Daniel
Miessler; the list itself is the UK NCSC's top 100,000 from Have I Been Pwned). Kept: entries
of 10 or more characters (shorter ones fail the length rule anyway) after Unicode NFKC and
lower-casing, without `$hex[…]` entries, sorted and unique. The check compares the NFKC,
lower-cased password against this list.

Regenerate:
```sh
curl -sfLO https://raw.githubusercontent.com/danielmiessler/SecLists/master/Passwords/Common-Credentials/100k-most-used-passwords-NCSC.txt
python3 -I -c 'import sys,unicodedata as u
s={u.normalize("NFKC",l.rstrip("\r\n")).lower() for l in open(sys.argv[1],encoding="utf-8",errors="replace")}
print("\n".join(sorted(p for p in s if 10<=len(p)<=256 and p.isprintable() and not p.startswith("$hex["))))' \
  100k-most-used-passwords-NCSC.txt > common-passwords.txt
```
