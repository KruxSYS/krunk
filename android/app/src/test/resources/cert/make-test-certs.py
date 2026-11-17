"""Makes the test certificates for CertDetailsTest (run once: python3 -I make-test-certs.py).

Fixed keys are not needed: the tests read names, dates, sizes and compare fingerprints with the
file's own SHA-256, so re-running only changes serials, keys and fingerprints.
"""
import datetime as dt
import ipaddress
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.x509.oid import NameOID

UTC = dt.timezone.utc


def name(**parts):
    oids = {"CN": NameOID.COMMON_NAME, "O": NameOID.ORGANIZATION_NAME, "C": NameOID.COUNTRY_NAME}
    return x509.Name([x509.NameAttribute(oids[k], v) for k, v in parts.items()])


def cert(path, subject, issuer, key, signer, start, end, sans=None, serial=None):
    b = (x509.CertificateBuilder().subject_name(subject).issuer_name(issuer)
         .public_key(key.public_key()).serial_number(serial or x509.random_serial_number())
         .not_valid_before(start).not_valid_after(end))
    if sans is not None:
        b = b.add_extension(x509.SubjectAlternativeName(sans), critical=False)
    c = b.sign(signer, hashes.SHA256())
    open(path, "wb").write(c.public_bytes(serialization.Encoding.DER))


ca_key = ec.generate_private_key(ec.SECP384R1())
ca = name(C="US", O="Krunk Test Authority", CN="Krunk Test CA R1")
rsa_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
ec_key = ec.generate_private_key(ec.SECP256R1())
y2020 = dt.datetime(2020, 1, 1, tzinfo=UTC)
y2100 = dt.datetime(2100, 1, 1, tzinfo=UTC)

# A normal site: CN and O, seven names (one international, one IP), signed by the CA.
dns = ["www.example.test", "example.test", "xn--bcher-kva.example", "*.cdn.example.test",
       "a.example.test", "b.example.test"]
cert("site.der", name(CN="www.example.test", O="Example Org"), ca, rsa_key, ca_key, y2020, y2100,
     [x509.DNSName(d) for d in dns] + [x509.IPAddress(ipaddress.ip_address("192.0.2.7"))],
     serial=0x00F00D)
# No CN (names only in the SAN), no O.
cert("no-cn.der", name(C="DE"), ca, ec_key, ca_key, y2020, y2100, [x509.DNSName("only-san.example")])
# Self-signed, CN only, no SAN; a comma and a plus in the CN.
cert("self.der", name(CN="Router, Inc+Co"), name(CN="Router, Inc+Co"), ec_key, ec_key, y2020, y2100)
# Expired in 2021, and valid only from 2099.
cert("expired.der", name(CN="old.example"), ca, ec_key, ca_key, y2020, dt.datetime(2021, 1, 1, tzinfo=UTC))
cert("future.der", name(CN="new.example"), ca, ec_key, ca_key, dt.datetime(2099, 1, 1, tzinfo=UTC), y2100)
