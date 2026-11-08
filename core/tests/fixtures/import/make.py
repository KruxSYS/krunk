#!/usr/bin/env python3
"""Makes the browser-profile fixtures for the import tests (core/src/import).

Everything is fake: made-up logins, a fake DPAPI (the test's Unprotect strips the FAKEDPAPI
prefix), deterministic salts. Run from the repository root:

    python3 -I core/tests/fixtures/import/make.py

Formats, as Krunk's parsers read them:
- Firefox key4.db: metaData (item1 = global salt, item2 = the "password-check" blob) and
  nssPrivate (a102 = key id, a11 = the encrypted SDR key), both PKCS#5 PBES2 (PBKDF2-HMAC-SHA256
  + AES-256-CBC, the PBKDF2 password being SHA-1(globalSalt + masterPassword)) or the legacy
  PBE-SHA1-3DES. logins.json fields are DER {keyId, {des-ede3-cbc, iv}, ciphertext}.
- Firefox places.sqlite: moz_places, moz_historyvisits, moz_bookmarks (root folders by guid).
- Chromium Login Data: logins rows with v10 (AES-256-GCM, key from Local State os_crypt.
  encrypted_key = "DPAPI" + blob), v20 (app-bound: counted, skipped) and legacy (plain DPAPI)
  password_value blobs. History: urls + visits (WebKit microseconds). Bookmarks: JSON roots.
- CSV: Chrome/Edge/Brave (name,url,username,password,note) and Firefox
  (url,username,password,httpRealm,formActionOrigin,guid,timeCreated,timeLastUsed,
  timePasswordChanged).
"""
import base64
import csv
import hashlib
import hmac
import json
import os
import sqlite3
import struct
import sys

from cryptography.hazmat.primitives import padding
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.decrepit.ciphers.algorithms import TripleDES

OUT = os.path.dirname(os.path.abspath(__file__))
FAKE_DPAPI = b"FAKEDPAPI"
MS_2024 = 1_700_000_000_000  # a time in ms
EPOCH_1601_S = 11_644_473_600


def det(n, tag):
    """Deterministic pseudo-random bytes."""
    out = b""
    i = 0
    while len(out) < n:
        out += hashlib.sha256(tag.encode() + bytes([i])).digest()
        i += 1
    return out[:n]


# ---------------------------------------------------------------- DER helpers
def der(tag, content):
    n = len(content)
    if n < 0x80:
        length = bytes([n])
    elif n < 0x100:
        length = b"\x81" + bytes([n])
    else:
        length = b"\x82" + struct.pack(">H", n)
    return bytes([tag]) + length + content


def seq(*items):
    return der(0x30, b"".join(items))


def octets(b):
    return der(0x04, b)


def integer(n):
    b = n.to_bytes((n.bit_length() + 8) // 8, "big") or b"\x00"
    return der(0x02, b)


def oid(dotted):
    parts = [int(p) for p in dotted.split(".")]
    body = bytes([40 * parts[0] + parts[1]])
    for p in parts[2:]:
        chunk = bytes([p & 0x7F])
        p >>= 7
        while p:
            chunk = bytes([0x80 | (p & 0x7F)]) + chunk
            p >>= 7
        body += chunk
    return der(0x06, body)


NULL = b"\x05\x00"
OID_PBES2 = "1.2.840.113549.1.5.13"
OID_PBKDF2 = "1.2.840.113549.1.5.12"
OID_HMAC_SHA256 = "1.2.840.113549.2.9"
OID_AES256_CBC = "2.16.840.1.101.3.4.1.42"
OID_PBE_SHA1_3DES = "1.2.840.113549.1.12.5.1.3"
OID_DES_EDE3_CBC = "1.2.840.113549.3.7"


# ---------------------------------------------------------------- Firefox NSS
def _pad(data, block):
    p = padding.PKCS7(block * 8).padder()
    return p.update(data) + p.finalize()


def aes_cbc_encrypt(key, iv, data):
    enc = Cipher(algorithms.AES(key), modes.CBC(iv)).encryptor()
    return enc.update(_pad(data, 16)) + enc.finalize()


def des3_cbc_encrypt(key, iv, data):
    enc = Cipher(TripleDES(key), modes.CBC(iv)).encryptor()
    return enc.update(_pad(data, 8)) + enc.finalize()


def pbes2_blob(global_salt, master_password, plaintext, tag):
    """The PBES2 (PBKDF2-HMAC-SHA256 + AES-256-CBC) blob NSS writes into key4.db."""
    entry_salt = det(32, tag + "-salt")
    iv14 = det(14, tag + "-iv")
    iterations = 10000
    password = hashlib.sha1(global_salt + master_password.encode()).digest()
    key = hashlib.pbkdf2_hmac("sha256", password, entry_salt, iterations, 32)
    iv = b"\x04\x0e" + iv14
    ciphertext = aes_cbc_encrypt(key, iv, plaintext)
    algo = seq(
        oid(OID_PBES2),
        seq(
            seq(oid(OID_PBKDF2), seq(octets(entry_salt), integer(iterations), integer(32), seq(oid(OID_HMAC_SHA256), NULL))),
            seq(oid(OID_AES256_CBC), octets(iv14)),
        ),
    )
    return seq(algo, octets(ciphertext))


def pbe_3des_blob(global_salt, master_password, plaintext, tag):
    """The legacy PBE-SHA1-3DES blob (old key4.db and key3.db)."""
    entry_salt = det(20, tag + "-salt")
    hp = hashlib.sha1(global_salt + master_password.encode()).digest()
    pes = entry_salt + b"\x00" * (20 - len(entry_salt))
    chp = hashlib.sha1(hp + entry_salt).digest()
    k1 = hmac.new(chp, pes + entry_salt, hashlib.sha1).digest()
    tk = hmac.new(chp, pes, hashlib.sha1).digest()
    k2 = hmac.new(chp, tk + entry_salt, hashlib.sha1).digest()
    k = k1 + k2
    key, iv = k[:24], k[-8:]
    ciphertext = des3_cbc_encrypt(key, iv, plaintext)
    algo = seq(oid(OID_PBE_SHA1_3DES), seq(octets(entry_salt), integer(1)))
    return seq(algo, octets(ciphertext))


KEY_ID = b"\xf8" + b"\x00" * 15
# Firefox 159 writes the SDR key id as f8 00..01 and encrypts login fields with AES-256-CBC
# (a 32-byte SDR key); older profiles use DES-EDE3-CBC with a 24-byte key.
KEY_ID_AES = b"\xf8" + b"\x00" * 14 + b"\x01"
FIREFOX_LOGINS = [
    ("https://login.example", "https://login.example", "fx-alice", "Fx-pass-alice-1"),
    ("https://shop.example", "https://shop.example/signin", "fx-bob", "Fx-pass-bob-2"),
    ("http://intranet.example", None, "fx-carol", "Fx-pass-carol-3"),  # httpRealm login
]


def sdr_encrypt(sdr_key, text, tag, aes):
    if aes:
        iv = det(16, tag + "-iv")
        ciphertext = aes_cbc_encrypt(sdr_key, iv, text.encode())
        return base64.b64encode(seq(octets(KEY_ID_AES), seq(oid(OID_AES256_CBC), octets(iv)), octets(ciphertext))).decode()
    iv = det(8, tag + "-iv")
    ciphertext = des3_cbc_encrypt(sdr_key, iv, text.encode())
    return base64.b64encode(seq(octets(KEY_ID), seq(oid(OID_DES_EDE3_CBC), octets(iv)), octets(ciphertext))).decode()


def make_firefox(name, master_password, legacy, aes=False):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    global_salt = det(20, name + "-global")
    sdr_key = det(32 if aes else 24, name + "-sdr")
    key_id = KEY_ID_AES if aes else KEY_ID
    blob = pbe_3des_blob if legacy else pbes2_blob
    path = os.path.join(d, "key4.db")
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    c.executescript(
        "CREATE TABLE metaData (id PRIMARY KEY UNIQUE ON CONFLICT REPLACE, item1, item2);"
        "CREATE TABLE nssPublic (id PRIMARY KEY UNIQUE ON CONFLICT ABORT, a0, a1, a2, a3, a11, a102);"
        "CREATE TABLE nssPrivate (id PRIMARY KEY UNIQUE ON CONFLICT ABORT, a0, a1, a2, a3, a11, a102);"
    )
    c.execute("INSERT INTO metaData VALUES ('password', ?, ?)", (global_salt, blob(global_salt, master_password, b"password-check", name + "-check")))
    c.execute("INSERT INTO nssPrivate (id, a11, a102) VALUES (1, ?, ?)", (blob(global_salt, master_password, sdr_key, name + "-a11"), key_id))
    c.commit()
    c.close()
    logins = []
    for i, (host, action, user, pw) in enumerate(FIREFOX_LOGINS, 1):
        logins.append({
            "id": i,
            "hostname": host,
            "httpRealm": None if action else "Intranet",
            "formSubmitURL": action,
            "usernameField": "user" if action else "",
            "passwordField": "pass" if action else "",
            "encryptedUsername": sdr_encrypt(sdr_key, user, f"{name}-u{i}", aes),
            "encryptedPassword": sdr_encrypt(sdr_key, pw, f"{name}-p{i}", aes),
            "guid": "{%08x-0000-4000-8000-000000000000}" % i,
            "encType": 1,
            "timeCreated": MS_2024 + i * 1000,
            "timeLastUsed": MS_2024 + i * 2000,
            "timePasswordChanged": MS_2024 + i * 1000,
            "timesUsed": i,
        })
    with open(os.path.join(d, "logins.json"), "w") as f:
        json.dump({"nextId": len(logins) + 1, "logins": logins, "potentiallyVulnerablePasswords": [], "dismissedBreachAlertsByLoginGUID": {}, "version": 3}, f, indent=1)
    make_places(os.path.join(d, "places.sqlite"))


def make_firefox_nohostname(name):
    """Krunk 0.7.0 (R25): a logins.json as some Firefox versions and tools write it: entries
    without `hostname` (the origin only in formSubmitURL), null fields, one that does not decrypt,
    one with an empty password, one that is not an object at all."""
    make_firefox(name, "", legacy=False, aes=True)
    d = os.path.join(OUT, name)
    sdr_key = det(32, name + "-sdr")
    enc = lambda text, tag: sdr_encrypt(sdr_key, text, f"{name}-{tag}", True)
    logins = [
        {"id": 1, "hostname": "https://login.example", "formSubmitURL": "https://login.example", "httpRealm": None,
         "encryptedUsername": enc("fx-alice", "u1"), "encryptedPassword": enc("Fx-pass-alice-1", "p1"), "timeCreated": MS_2024},
        {"id": 2, "formSubmitURL": "https://accounts.forms.example/signin", "httpRealm": None,
         "encryptedUsername": enc("no-host-user", "u2"), "encryptedPassword": enc("No-host-pass-2", "p2")},
        {"id": 3, "hostname": None, "formSubmitURL": None, "httpRealm": "https://realm.example",
         "encryptedUsername": enc("realm-user", "u3"), "encryptedPassword": enc("Realm-pass-3", "p3")},
        {"id": 4, "hostname": None, "formSubmitURL": None, "httpRealm": "Intranet",
         "encryptedUsername": enc("nowhere", "u4"), "encryptedPassword": enc("Nowhere-pass-4", "p4")},
        {"id": 5, "hostname": "https://broken.example", "encryptedUsername": "bm90IGEgYmxvYg==", "encryptedPassword": "bm90IGEgYmxvYg=="},
        {"id": 6, "hostname": "https://empty.example", "encryptedUsername": enc("empty-user", "u6"), "encryptedPassword": enc("", "p6")},
        {"id": 7, "hostname": 5, "encryptedUsername": 1},
        "not an entry",
    ]
    with open(os.path.join(d, "logins.json"), "w") as f:
        json.dump({"nextId": 9, "logins": logins, "version": 3}, f, indent=1)
    # No history or bookmarks needed.
    os.remove(os.path.join(d, "places.sqlite"))


def make_places(path):
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    c.executescript(
        "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, rev_host LONGVARCHAR,"
        " visit_count INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0 NOT NULL, typed INTEGER DEFAULT 0 NOT NULL,"
        " frecency INTEGER DEFAULT -1 NOT NULL, last_visit_date INTEGER, guid TEXT, foreign_count INTEGER DEFAULT 0 NOT NULL,"
        " url_hash INTEGER DEFAULT 0 NOT NULL, description TEXT, preview_image_url TEXT, site_name TEXT, origin_id INTEGER,"
        " recalc_frecency INTEGER NOT NULL DEFAULT 0, alt_frecency INTEGER, recalc_alt_frecency INTEGER NOT NULL DEFAULT 0);"
        "CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, from_visit INTEGER, place_id INTEGER, visit_date INTEGER,"
        " visit_type INTEGER, session INTEGER, source INTEGER DEFAULT 0 NOT NULL, triggeringPlaceId INTEGER);"
        "CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER,"
        " position INTEGER, title LONGVARCHAR, keyword_id INTEGER, folder_type TEXT, dateAdded INTEGER, lastModified INTEGER,"
        " guid TEXT, syncStatus INTEGER NOT NULL DEFAULT 0, syncChangeCounter INTEGER NOT NULL DEFAULT 1);"
    )
    us = MS_2024 * 1000  # microseconds since the epoch
    places = [
        (1, "https://news.example/", "News", 3, us + 1_000_000),
        (2, "https://docs.example/guide", "The guide", 1, us + 2_000_000),
        (3, "https://login.example/", "Login", 1, us + 3_000_000),
        (4, "place:sort=8&maxResults=10", "Most visited", 0, None),  # a place: query, not a page
    ]
    for pid, url, title, count, last in places:
        c.execute("INSERT INTO moz_places (id, url, title, visit_count, last_visit_date, hidden) VALUES (?,?,?,?,?,?)", (pid, url, title, count, last, 1 if url.startswith("place:") else 0))
    visits = [(1, us + 500_000), (1, us + 800_000), (1, us + 1_000_000), (2, us + 2_000_000), (3, us + 3_000_000)]
    for i, (pid, when) in enumerate(visits, 1):
        c.execute("INSERT INTO moz_historyvisits (id, place_id, visit_date, visit_type) VALUES (?,?,?,1)", (i, pid, when))
    # Roots: 1 root, 2 menu, 3 toolbar, 4 tags, 5 unfiled, 6 mobile.
    roots = [(1, 2, 0, 0, "", "root________"), (2, 2, 1, 0, "menu", "menu________"), (3, 2, 1, 1, "toolbar", "toolbar_____"),
             (4, 2, 1, 2, "tags", "tags________"), (5, 2, 1, 3, "unfiled", "unfiled_____"), (6, 2, 1, 4, "mobile", "mobile______")]
    for bid, typ, parent, pos, title, guid in roots:
        c.execute("INSERT INTO moz_bookmarks (id, type, parent, position, title, dateAdded, lastModified, guid) VALUES (?,?,?,?,?,?,?,?)", (bid, typ, parent, pos, title, us, us, guid))
    marks = [
        (7, 1, 1, 3, 0, "News", us + 10),          # toolbar bookmark
        (8, 2, None, 3, 1, "Work", us + 20),       # toolbar folder
        (9, 1, 2, 8, 0, "The guide", us + 30),     # in Work
        (10, 1, 3, 5, 0, "Login", us + 40),        # unfiled bookmark
        (11, 1, 4, 2, 0, "Most visited", us + 50), # place: query in the menu: skipped
    ]
    for bid, typ, fk, parent, pos, title, added in marks:
        c.execute("INSERT INTO moz_bookmarks (id, type, fk, parent, position, title, dateAdded, lastModified, guid) VALUES (?,?,?,?,?,?,?,?,?)", (bid, typ, fk, parent, pos, title, added, added, "guid%08d" % bid))
    c.commit()
    c.close()


# ---------------------------------------------------------------- Chromium
CHROME_KEY = det(32, "chrome-oscrypt-key")
CHROME_LOGINS = [
    ("https://login.example/", "https://login.example/", "cr-alice", "Cr-pass-alice-1", "v10"),
    ("https://shop.example/signin", "https://shop.example/", "cr-bob", "Cr-pass-bob-2", "v10"),
    ("https://old.example/", "https://old.example/", "cr-dave", "Cr-pass-dave-4", "legacy"),
    ("https://bound.example/", "https://bound.example/", "cr-erin", "Cr-pass-erin-5", "v20"),
    ("https://never.example/", "https://never.example/", "", "", "blocked"),  # never-save row
]


def webkit_us(ms):
    return (ms // 1000 + EPOCH_1601_S) * 1_000_000 + (ms % 1000) * 1000


def chrome_password_blob(kind, password, tag):
    if kind == "v10":
        nonce = det(12, tag + "-nonce")
        return b"v10" + nonce + AESGCM(CHROME_KEY).encrypt(nonce, password.encode(), None)
    if kind == "v20":
        return b"v20" + det(12, tag + "-nonce") + det(len(password) + 16, tag + "-opaque")
    if kind == "legacy":
        return FAKE_DPAPI + password.encode()
    return b""


def make_chromium(name):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "Local State"), "w") as f:
        json.dump({
            "os_crypt": {
                "encrypted_key": base64.b64encode(b"DPAPI" + FAKE_DPAPI + CHROME_KEY).decode(),
                "app_bound_encrypted_key": base64.b64encode(b"APPB" + det(64, name + "-appb")).decode(),
            },
            "profile": {"info_cache": {"Default": {"name": "Person 1"}}},
        }, f, indent=1)
    prof = os.path.join(d, "Default")
    os.makedirs(prof, exist_ok=True)
    path = os.path.join(prof, "Login Data")
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    c.executescript(
        "CREATE TABLE logins (origin_url VARCHAR NOT NULL, action_url VARCHAR, username_element VARCHAR, username_value VARCHAR,"
        " password_element VARCHAR, password_value BLOB, submit_element VARCHAR, signon_realm VARCHAR NOT NULL,"
        " date_created INTEGER NOT NULL, blacklisted_by_user INTEGER NOT NULL, scheme INTEGER NOT NULL, password_type INTEGER,"
        " times_used INTEGER, form_data BLOB, display_name VARCHAR, icon_url VARCHAR, federation_url VARCHAR,"
        " skip_zero_click INTEGER, generation_upload_status INTEGER, possible_username_pairs BLOB, id INTEGER PRIMARY KEY AUTOINCREMENT,"
        " date_last_used INTEGER NOT NULL DEFAULT 0, moving_blocked_for BLOB, date_password_modified INTEGER NOT NULL DEFAULT 0,"
        " sender_email VARCHAR, sender_name VARCHAR, date_received INTEGER, sharing_notification_displayed INTEGER NOT NULL DEFAULT 0,"
        " keychain_identifier BLOB, sender_profile_image_url VARCHAR, UNIQUE (origin_url, username_element, username_value, password_element, signon_realm));"
        "CREATE TABLE meta (key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);"
        "INSERT INTO meta VALUES ('version', '41');"
    )
    for i, (origin, realm, user, pw, kind) in enumerate(CHROME_LOGINS, 1):
        c.execute(
            "INSERT INTO logins (origin_url, action_url, username_element, username_value, password_element, password_value, signon_realm,"
            " date_created, blacklisted_by_user, scheme, date_last_used, date_password_modified) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            (origin, origin, "user", user, "pass", chrome_password_blob(kind, pw, f"{name}-{i}"), realm,
             webkit_us(MS_2024 + i * 1000), 1 if kind == "blocked" else 0, 0, webkit_us(MS_2024 + i * 2000), webkit_us(MS_2024 + i * 1000)),
        )
    c.commit()
    c.close()
    path = os.path.join(prof, "History")
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    c.executescript(
        "CREATE TABLE urls (id INTEGER PRIMARY KEY AUTOINCREMENT, url LONGVARCHAR, title LONGVARCHAR, visit_count INTEGER DEFAULT 0 NOT NULL,"
        " typed_count INTEGER DEFAULT 0 NOT NULL, last_visit_time INTEGER NOT NULL, hidden INTEGER DEFAULT 0 NOT NULL);"
        "CREATE TABLE visits (id INTEGER PRIMARY KEY, url INTEGER NOT NULL, visit_time INTEGER NOT NULL, from_visit INTEGER,"
        " external_referrer_url TEXT, transition INTEGER DEFAULT 0 NOT NULL, segment_id INTEGER, visit_duration INTEGER DEFAULT 0 NOT NULL,"
        " incremented_omnibox_typed_score BOOLEAN DEFAULT FALSE NOT NULL, opener_visit INTEGER, originator_cache_guid TEXT,"
        " originator_visit_id INTEGER, originator_from_visit INTEGER, originator_opener_visit INTEGER, is_known_to_sync BOOLEAN DEFAULT FALSE NOT NULL,"
        " consider_for_ntp_most_visited BOOLEAN DEFAULT FALSE NOT NULL, visited_link_id INTEGER DEFAULT 0 NOT NULL, app_id TEXT);"
        "CREATE TABLE meta (key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);"
        "INSERT INTO meta VALUES ('version', '74');"
    )
    urls = [
        (1, "https://news.example/", "News", 2, 0),
        (2, "https://docs.example/guide", "The guide", 1, 0),
        (3, "chrome://settings/", "Settings", 1, 0),  # not web content: skipped
        (4, "https://hidden.example/", "Hidden", 1, 1),  # hidden: skipped
    ]
    visits = [(1, MS_2024 + 1000), (1, MS_2024 + 5000), (2, MS_2024 + 2000), (3, MS_2024 + 3000), (4, MS_2024 + 4000)]
    for uid, url, title, count, hidden in urls:
        last = max(ms for u, ms in visits if u == uid)
        c.execute("INSERT INTO urls (id, url, title, visit_count, last_visit_time, hidden) VALUES (?,?,?,?,?,?)", (uid, url, title, count, webkit_us(last), hidden))
    for i, (uid, ms) in enumerate(visits, 1):
        c.execute("INSERT INTO visits (id, url, visit_time, transition) VALUES (?,?,?,805306368)", (i, uid, webkit_us(ms)))
    c.commit()
    c.close()

    def node(id_, name_, url=None, children=None, added=MS_2024):
        n = {"date_added": str(webkit_us(added)), "guid": "%08x-0000-4000-8000-000000000000" % id_, "id": str(id_), "name": name_}
        if url is None:
            n.update({"type": "folder", "children": children or [], "date_modified": str(webkit_us(added))})
        else:
            n.update({"type": "url", "url": url})
        return n

    bookmarks = {
        "checksum": "0",
        "roots": {
            "bookmark_bar": node(1, "Bookmarks bar", children=[
                node(5, "News", "https://news.example/", added=MS_2024 + 10),
                node(6, "Work", children=[node(7, "The guide", "https://docs.example/guide", added=MS_2024 + 30)], added=MS_2024 + 20),
            ]),
            "other": node(2, "Other bookmarks", children=[node(8, "Login", "https://login.example/", added=MS_2024 + 40), node(9, "Script", "javascript:void(0)", added=MS_2024 + 50)]),
            "synced": node(3, "Mobile bookmarks", children=[]),
        },
        "version": 1,
    }
    with open(os.path.join(prof, "Bookmarks"), "w") as f:
        json.dump(bookmarks, f, indent=1)


# ---------------------------------------------------------------- CSV
def make_csv():
    with open(os.path.join(OUT, "chrome.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["name", "url", "username", "password", "note"])
        w.writerow(["login.example", "https://login.example/", "csv-alice", "Csv-pass-alice-1", ""])
        w.writerow(["shop.example", "https://shop.example/signin", "csv-bob", "Csv,pass \"bob\" 2", "two lines\nof note"])
        w.writerow(["nowhere", "not a url", "csv-nobody", "x", ""])  # invalid URL: skipped
    with open(os.path.join(OUT, "firefox.csv"), "w", newline="") as f:
        w = csv.writer(f, quoting=csv.QUOTE_ALL)
        w.writerow(["url", "username", "password", "httpRealm", "formActionOrigin", "guid", "timeCreated", "timeLastUsed", "timePasswordChanged"])
        w.writerow(["https://login.example", "csv-alice", "Csv-pass-alice-1", "", "https://login.example", "{1}", str(MS_2024), str(MS_2024), str(MS_2024)])
        w.writerow(["http://intranet.example", "csv-carol", "Csv-pass-carol-3", "Intranet", "", "{3}", str(MS_2024), str(MS_2024), str(MS_2024)])


def key4_tables(c):
    c.executescript(
        "CREATE TABLE metaData (id PRIMARY KEY UNIQUE ON CONFLICT REPLACE, item1, item2);"
        "CREATE TABLE nssPublic (id PRIMARY KEY UNIQUE ON CONFLICT ABORT, a0, a1, a2, a3, a11, a102);"
        "CREATE TABLE nssPrivate (id PRIMARY KEY UNIQUE ON CONFLICT ABORT, a0, a1, a2, a3, a11, a102);"
    )


def make_firefox_twokeys(name):
    """Krunk 0.7.2 (R12): a profile that went from 3DES to AES. key4.db holds an old 3DES SDR key
    (row 1, listed first) and the newer AES key (row 2); logins.json has logins under both. No
    primary password. Also firefox-wal: the same two keys, the second one only in key4.db-wal
    (Firefox running, not yet checkpointed)."""
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    global_salt = det(20, name + "-global")
    old_key = det(24, name + "-sdr-old")
    new_key = det(32, name + "-sdr-new")
    path = os.path.join(d, "key4.db")
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    key4_tables(c)
    c.execute("INSERT INTO metaData VALUES ('password', ?, ?)", (global_salt, pbes2_blob(global_salt, "", b"password-check", name + "-check")))
    c.execute("INSERT INTO nssPrivate (id, a11, a102) VALUES (1, ?, ?)", (pbes2_blob(global_salt, "", old_key, name + "-a11-old"), KEY_ID))
    c.execute("INSERT INTO nssPrivate (id, a11, a102) VALUES (2, ?, ?)", (pbes2_blob(global_salt, "", new_key, name + "-a11-new"), KEY_ID_AES))
    c.commit()
    c.close()
    rows = [
        ("https://new-aes.example", "fx-new-1", "Fx-new-pass-1", True),
        ("https://old-3des.example", "fx-old-2", "Fx-old-pass-2", False),
        ("https://new-aes.example", "fx-new-3", "Fx-new-pass-3", True),
    ]
    logins = []
    for i, (host, user, pw, aes) in enumerate(rows, 1):
        key = new_key if aes else old_key
        logins.append({
            "id": i, "hostname": host, "httpRealm": None, "formSubmitURL": host,
            "usernameField": "user", "passwordField": "pass",
            "encryptedUsername": sdr_encrypt(key, user, f"{name}-u{i}", aes),
            "encryptedPassword": sdr_encrypt(key, pw, f"{name}-p{i}", aes),
            "guid": "{%08x-0000-4000-8000-0000000000aa}" % i, "encType": 1,
            "timeCreated": MS_2024 + i * 1000, "timeLastUsed": MS_2024 + i * 2000,
            "timePasswordChanged": MS_2024 + i * 1000, "timesUsed": i,
        })
    with open(os.path.join(d, "logins.json"), "w") as f:
        json.dump({"nextId": len(logins) + 1, "logins": logins, "version": 3}, f, indent=1)

    # firefox-wal: built in a temporary folder and copied while the connection is still open
    # (closing the last connection would move the log into the database).
    import shutil
    import tempfile
    w = os.path.join(OUT, "firefox-wal")
    os.makedirs(w, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tp = os.path.join(tmp, "key4.db")
        c = sqlite3.connect(tp)
        c.execute("PRAGMA journal_mode=WAL")
        c.execute("PRAGMA wal_autocheckpoint=0")
        key4_tables(c)
        c.execute("INSERT INTO metaData VALUES ('password', ?, ?)", (global_salt, pbes2_blob(global_salt, "", b"password-check", name + "-check")))
        c.execute("INSERT INTO nssPrivate (id, a11, a102) VALUES (1, ?, ?)", (pbes2_blob(global_salt, "", old_key, name + "-a11-old"), KEY_ID))
        c.commit()
        c.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        c.execute("INSERT INTO nssPrivate (id, a11, a102) VALUES (2, ?, ?)", (pbes2_blob(global_salt, "", new_key, name + "-a11-new"), KEY_ID_AES))
        c.commit()
        shutil.copy(tp, os.path.join(w, "key4.db"))
        shutil.copy(tp + "-wal", os.path.join(w, "key4.db-wal"))
        c.close()


def main():
    if sys.argv[1:] == ["firefox-nohostname"]:
        make_firefox_nohostname("firefox-nohostname")
        return
    if sys.argv[1:] == ["firefox-twokeys"]:
        make_firefox_twokeys("firefox-twokeys")
        return
    make_firefox("firefox-nomaster", "", legacy=False, aes=True)
    make_firefox("firefox-master", "fx-master-pw", legacy=False)
    make_firefox("firefox-legacy", "fx-master-pw", legacy=True)
    make_chromium("chrome")
    make_csv()
    make_firefox_nohostname("firefox-nohostname")
    make_firefox_twokeys("firefox-twokeys")
    print("fixtures written to", OUT)


if __name__ == "__main__":
    sys.exit(main())
