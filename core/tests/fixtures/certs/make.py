#!/usr/bin/env python3
"""Makes the X.509 fixtures for core/src/certinfo.rs tests (fake, for tests only).

    python3 -I core/tests/fixtures/certs/make.py

root.der          EC P-256 self-signed CA "Krunk Test Root"
intermediate.der  RSA 2048 CA signed by the root
leaf.der          RSA 2048 server cert signed by the intermediate: CN test.example,
                  SANs test.example, *.test.example, 192.168.1.5; valid 2025-01-01..2027-01-01
expired.der       EC server cert signed by the intermediate, valid 2020-01-01..2021-01-01
selfsigned.der    EC self-signed "localhost", no SANs apart from DNS localhost
"""
import datetime as dt
import pathlib

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.x509.oid import NameOID

HERE = pathlib.Path(__file__).resolve().parent
UTC = dt.timezone.utc


def name(cn, org):
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn), x509.NameAttribute(NameOID.ORGANIZATION_NAME, org),
                      x509.NameAttribute(NameOID.COUNTRY_NAME, "DE")])


def build(subject, issuer, key, signer, start, end, sans=None, ca=False, serial=1):
    b = (x509.CertificateBuilder().subject_name(subject).issuer_name(issuer).public_key(key.public_key())
         .serial_number(serial).not_valid_before(start).not_valid_after(end)
         .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True))
    if sans:
        b = b.add_extension(x509.SubjectAlternativeName(sans), critical=False)
    return b.sign(signer, hashes.SHA256())


def write(n, cert):
    (HERE / n).write_bytes(cert.public_bytes(serialization.Encoding.DER))


root_key = ec.generate_private_key(ec.SECP256R1())
root_name = name("Krunk Test Root", "Krunk Tests")
root = build(root_name, root_name, root_key, root_key, dt.datetime(2024, 1, 1, tzinfo=UTC), dt.datetime(2034, 1, 1, tzinfo=UTC), ca=True, serial=1)
write("root.der", root)

inter_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
inter_name = name("Krunk Test Intermediate", "Krunk Tests")
inter = build(inter_name, root_name, inter_key, root_key, dt.datetime(2024, 6, 1, tzinfo=UTC), dt.datetime(2030, 6, 1, tzinfo=UTC), ca=True, serial=2)
write("intermediate.der", inter)

leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
leaf = build(name("test.example", "Example Site"), inter_name, leaf_key, inter_key,
             dt.datetime(2025, 1, 1, tzinfo=UTC), dt.datetime(2027, 1, 1, tzinfo=UTC),
             sans=[x509.DNSName("test.example"), x509.DNSName("*.test.example"), x509.IPAddress(__import__("ipaddress").ip_address("192.168.1.5"))],
             serial=3)
write("leaf.der", leaf)

exp_key = ec.generate_private_key(ec.SECP384R1())
expired = build(name("old.example", "Example Site"), inter_name, exp_key, inter_key,
                dt.datetime(2020, 1, 1, tzinfo=UTC), dt.datetime(2021, 1, 1, tzinfo=UTC), sans=[x509.DNSName("old.example")], serial=4)
write("expired.der", expired)

self_key = ec.generate_private_key(ec.SECP256R1())
self_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "localhost")])
selfsigned = build(self_name, self_name, self_key, self_key, dt.datetime(2025, 1, 1, tzinfo=UTC), dt.datetime(2035, 1, 1, tzinfo=UTC),
                   sans=[x509.DNSName("localhost")], serial=5)
write("selfsigned.der", selfsigned)
print("wrote", [p.name for p in sorted(HERE.glob("*.der"))])
