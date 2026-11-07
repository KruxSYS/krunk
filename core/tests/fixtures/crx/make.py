"""Makes the CRX3 test packages for core::ext::verify_crx (Krunk 0.7.0):
  signed.crx       a zip signed by its own (throwaway) 2048-bit RSA key, crx_id = that key
  tampered.crx     the same with one byte of the zip changed
  wrong-key.crx    signed by a second key while crx_id names the first
Run from this folder: python3 make.py (needs openssl). The keys are generated anew and thrown
away; only the packages are kept.
"""
import hashlib
import io
import os
import struct
import subprocess
import tempfile
import zipfile


def varint(n):
    out = b""
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out += bytes([b | 0x80])
        else:
            return out + bytes([b])


def field(num, data):
    return varint((num << 3) | 2) + varint(len(data)) + data


def key(tmp, name):
    pem = os.path.join(tmp, name + ".pem")
    subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", pem], check=True, capture_output=True)
    der = subprocess.run(["openssl", "pkey", "-in", pem, "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout
    return pem, der


def sign(pem, data, tmp):
    msg = os.path.join(tmp, "msg")
    open(msg, "wb").write(data)
    return subprocess.run(["openssl", "dgst", "-sha256", "-sign", pem, msg], check=True, capture_output=True).stdout


def crx(zip_bytes, id_key_der, signer_pem, signer_der, tmp, tamper=False):
    crx_id = hashlib.sha256(id_key_der).digest()[:16]
    signed_data = field(1, crx_id)
    message = b"CRX3 SignedData\x00" + struct.pack("<I", len(signed_data)) + signed_data + zip_bytes
    sig = sign(signer_pem, message, tmp)
    header = field(2, field(1, signer_der) + field(2, sig)) + field(10000, signed_data)
    body = bytearray(zip_bytes)
    if tamper:
        body[40] ^= 1
    return b"Cr24" + struct.pack("<I", 3) + struct.pack("<I", len(header)) + header + bytes(body)


def main():
    z = io.BytesIO()
    with zipfile.ZipFile(z, "w", zipfile.ZIP_STORED) as f:
        f.writestr("manifest.json", '{"manifest_version": 3, "name": "Krunk CRX fixture", "version": "1.0", "permissions": ["storage"], "host_permissions": ["https://*.example.com/*"]}')
    zip_bytes = z.getvalue()
    with tempfile.TemporaryDirectory() as tmp:
        pem, der = key(tmp, "a")
        pem2, der2 = key(tmp, "b")
        open("signed.crx", "wb").write(crx(zip_bytes, der, pem, der, tmp))
        open("tampered.crx", "wb").write(crx(zip_bytes, der, pem, der, tmp, tamper=True))
        # crx_id names key a, but key b signed it (and only b's proof is in the header).
        open("wrong-key.crx", "wb").write(crx(zip_bytes, der, pem2, der2, tmp))


if __name__ == "__main__":
    main()
