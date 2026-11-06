//! Site information for the lock popup (Krunk 0.5.0): what the engine reports about the
//! connection and the certificate, read from the DER the engine hands over. A small,
//! bounded DER reader is enough for the fields shown (subject, issuer, validity, the
//! subject alternative names, the algorithms); nothing here verifies anything, the engine
//! already did (`cert_status`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Most bytes of DER looked at (a chain of certificates is a few KB; this is defence).
pub const MAX_DER: usize = 64 * 1024;

/// What a certificate says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertSummary {
    pub subject: String,
    pub subject_org: String,
    pub issuer: String,
    pub issuer_org: String,
    /// Unix seconds.
    pub not_before: i64,
    pub not_after: i64,
    pub sans: Vec<String>,
    pub key_algorithm: String,
    pub signature_algorithm: String,
    /// "AA:BB:…" of the whole DER.
    pub sha256: String,
    pub serial: String,
}

/// The connection of the page, as the popup explains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SiteKind {
    /// A built-in krunk:// page.
    Internal,
    File,
    Http,
    Https,
    Other,
}

/// What the engine reports for the page's connection (plain data from `CefSSLStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SslReport {
    /// The URL the report is for (the navigation entry's, which can differ from the tab's
    /// while an interstitial shows); empty when unknown.
    pub url: String,
    pub secure: bool,
    /// `cef_cert_status_t` bits.
    pub cert_status: u32,
    /// `cef_ssl_version_t`.
    pub version: i32,
    /// `cef_ssl_content_status_t` bits.
    pub content_status: u32,
    /// The certificate chain, DER, leaf first.
    pub chain: Vec<Vec<u8>>,
}

/// The lock popup's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub host: String,
    pub kind: SiteKind,
    pub secure: bool,
    /// Short sentences the popup shows prominently (certificate errors, mixed content).
    pub warnings: Vec<String>,
    pub protocol: Option<String>,
    pub cipher: Option<String>,
    /// Leaf first.
    pub certs: Vec<CertSummary>,
}

pub fn classify_url(url: &str) -> SiteKind {
    if crate::pages::Page::from_url(url).is_some() {
        return SiteKind::Internal;
    }
    match url.split(':').next() {
        Some("https") => SiteKind::Https,
        Some("http") => SiteKind::Http,
        Some("file") => SiteKind::File,
        _ => SiteKind::Other,
    }
}

/// Builds the popup's content from the page URL and the engine's report (none for pages
/// that have no navigation entry yet).
pub fn site_info(url: &str, report: Option<&SslReport>) -> SiteInfo {
    let url = report.map(|r| r.url.as_str()).filter(|u| !u.is_empty()).unwrap_or(url);
    let kind = classify_url(url);
    let host = crate::urlclean::host_of(url).unwrap_or_default();
    let mut info =
        SiteInfo { host, kind, secure: false, warnings: Vec::new(), protocol: None, cipher: None, certs: Vec::new() };
    if kind != SiteKind::Https {
        return info;
    }
    let Some(r) = report else { return info };
    info.secure = r.secure;
    info.protocol = ssl_version_name(r.version).map(str::to_string);
    for name in cert_status_names(r.cert_status) {
        info.warnings.push(format!("Certificate error: {name}."));
    }
    if r.content_status & 2 != 0 {
        info.warnings.push("This page ran insecure scripts or styles (mixed content).".into());
    } else if r.content_status & 1 != 0 {
        info.warnings.push("This page shows insecure images or media (mixed content).".into());
    }
    info.certs = r.chain.iter().take(8).filter_map(|der| parse_der(der).ok()).collect();
    if info.certs.is_empty() && r.secure {
        info.warnings.push("The engine reported no certificate for this page.".into());
    }
    info
}

/// `cef_ssl_version_t` → name.
pub fn ssl_version_name(v: i32) -> Option<&'static str> {
    Some(match v {
        1 => "SSL 2.0",
        2 => "SSL 3.0",
        3 => "TLS 1.0",
        4 => "TLS 1.1",
        5 => "TLS 1.2",
        6 => "TLS 1.3",
        7 => "QUIC",
        _ => return None,
    })
}

/// The error bits of `cef_cert_status_t` as sentences (the informational bits are skipped).
pub fn cert_status_names(bits: u32) -> Vec<String> {
    const NAMES: [(u32, &str); 12] = [
        (1, "the certificate's name does not match the site"),
        (2, "the certificate has expired or is not yet valid"),
        (4, "the certificate authority is not trusted"),
        (64, "the certificate was revoked"),
        (128, "the certificate is invalid"),
        (256, "the certificate uses a weak signature algorithm"),
        (1024, "the certificate's name is not unique"),
        (2048, "the certificate uses a weak key"),
        (8192, "the site's pinned key is missing from the chain"),
        (16384, "the certificate violates a name constraint"),
        (32768, "the certificate's validity period is too long"),
        (1 << 20, "the certificate fails Certificate Transparency"),
    ];
    NAMES.iter().filter(|(b, _)| bits & b != 0).map(|(_, n)| (*n).to_string()).collect()
}

// ------------------------------------------------------------------ DER

#[derive(Debug, Clone, Copy)]
struct Tlv<'a> {
    tag: u8,
    body: &'a [u8],
}

/// Reads one TLV at the start of `d`; returns it and the rest.
fn tlv(d: &[u8]) -> Result<(Tlv<'_>, &[u8]), String> {
    let (&tag, rest) = d.split_first().ok_or("truncated")?;
    let (&first, rest) = rest.split_first().ok_or("truncated length")?;
    let (len, rest) = if first & 0x80 == 0 {
        (usize::from(first), rest)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return Err("bad length".into());
        }
        let len = rest[..n].iter().fold(0usize, |a, &b| (a << 8) | usize::from(b));
        (len, &rest[n..])
    };
    if rest.len() < len {
        return Err("truncated body".into());
    }
    Ok((Tlv { tag, body: &rest[..len] }, &rest[len..]))
}

/// The TLVs inside a SEQUENCE/SET body, in order (at most 64).
fn children(body: &[u8]) -> Result<Vec<Tlv<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (t, r) = tlv(rest)?;
        out.push(t);
        rest = r;
        if out.len() > 64 {
            return Err("too many elements".into());
        }
    }
    Ok(out)
}

fn oid_string(body: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut acc: u64 = 0;
    for (i, &b) in body.iter().enumerate() {
        acc = (acc << 7) | u64::from(b & 0x7f);
        if b & 0x80 == 0 {
            if parts.is_empty() && i < 4 {
                parts.push((acc / 40).min(2));
                parts.push(if acc / 40 >= 2 { acc - 80 } else { acc % 40 });
            } else {
                parts.push(acc);
            }
            acc = 0;
        }
    }
    parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

fn algorithm_name(oid: &str) -> String {
    match oid {
        "1.2.840.113549.1.1.1" => "RSA",
        "1.2.840.10045.2.1" => "EC",
        "1.3.101.112" => "Ed25519",
        "1.2.840.113549.1.1.11" => "SHA-256 with RSA",
        "1.2.840.113549.1.1.12" => "SHA-384 with RSA",
        "1.2.840.113549.1.1.13" => "SHA-512 with RSA",
        "1.2.840.113549.1.1.10" => "RSA-PSS",
        "1.2.840.10045.4.3.2" => "ECDSA with SHA-256",
        "1.2.840.10045.4.3.3" => "ECDSA with SHA-384",
        "1.2.840.10045.4.3.4" => "ECDSA with SHA-512",
        "1.2.840.10045.3.1.7" => "P-256",
        "1.3.132.0.34" => "P-384",
        "1.3.132.0.35" => "P-521",
        other => return other.to_string(),
    }
    .to_string()
}

/// An AlgorithmIdentifier: the algorithm, with the curve for EC keys.
fn algorithm(t: Tlv<'_>) -> Result<String, String> {
    let parts = children(t.body)?;
    let oid = parts.first().filter(|p| p.tag == 0x06).ok_or("no algorithm OID")?;
    let mut name = algorithm_name(&oid_string(oid.body));
    if let Some(curve) = parts.get(1).filter(|p| p.tag == 0x06) {
        name = format!("{name} {}", algorithm_name(&oid_string(curve.body)));
    }
    Ok(name)
}

/// The CN and O of a Name.
fn name_parts(t: Tlv<'_>) -> Result<(String, String), String> {
    let (mut cn, mut org) = (String::new(), String::new());
    for set in children(t.body)? {
        for attr in children(set.body)? {
            let kv = children(attr.body)?;
            let (Some(k), Some(v)) = (kv.first(), kv.get(1)) else { continue };
            let text =
                String::from_utf8_lossy(v.body).chars().filter(|c| !c.is_control()).take(200).collect::<String>();
            match oid_string(k.body).as_str() {
                "2.5.4.3" => cn = text,
                "2.5.4.10" => org = text,
                _ => {}
            }
        }
    }
    Ok((cn, org))
}

/// UTCTime / GeneralizedTime → Unix seconds.
fn time(t: Tlv<'_>) -> Result<i64, String> {
    let s = std::str::from_utf8(t.body).map_err(|_| "bad time")?;
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let (year, rest) = match t.tag {
        0x17 if digits.len() >= 10 => {
            let yy: i64 = digits[..2].parse().map_err(|_| "bad year")?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, &digits[2..])
        }
        0x18 if digits.len() >= 12 => (digits[..4].parse().map_err(|_| "bad year")?, &digits[4..]),
        _ => return Err("bad time".into()),
    };
    let num = |i: usize| -> Result<i64, String> {
        rest.get(i..i + 2).ok_or("short time")?.parse().map_err(|_| "bad time".into())
    };
    let (month, day, hour, min) = (num(0)?, num(2)?, num(4)?, num(6)?);
    let sec = if rest.len() >= 10 { num(8)? } else { 0 };
    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3600 + min * 60 + sec)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The subjectAltName entries (DNS and IP) of an extensions block.
fn sans(ext_block: Tlv<'_>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let seq = children(ext_block.body)?;
    let Some(list) = seq.first() else { return Ok(out) };
    for ext in children(list.body)? {
        let parts = children(ext.body)?;
        let Some(oid) = parts.first() else { continue };
        if oid_string(oid.body) != "2.5.29.17" {
            continue;
        }
        let Some(value) = parts.iter().find(|p| p.tag == 0x04) else { continue };
        let (names, _) = tlv(value.body)?;
        for g in children(names.body)? {
            match g.tag {
                0x82 => {
                    out.push(String::from_utf8_lossy(g.body).chars().filter(|c| !c.is_control()).take(253).collect())
                }
                0x87 if g.body.len() == 4 => out.push(g.body.iter().map(u8::to_string).collect::<Vec<_>>().join(".")),
                0x87 if g.body.len() == 16 => out.push(
                    g.body
                        .chunks(2)
                        .map(|c| format!("{:x}", (u16::from(c[0]) << 8) | u16::from(c[1])))
                        .collect::<Vec<_>>()
                        .join(":"),
                ),
                _ => {}
            }
            if out.len() >= 64 {
                break;
            }
        }
    }
    Ok(out)
}

/// Reads the fields shown in the popup from a DER certificate.
pub fn parse_der(der: &[u8]) -> Result<CertSummary, String> {
    if der.len() > MAX_DER {
        return Err("certificate too large".into());
    }
    let (cert, _) = tlv(der)?;
    if cert.tag != 0x30 {
        return Err("not a certificate".into());
    }
    let outer = children(cert.body)?;
    let tbs = outer.first().filter(|t| t.tag == 0x30).ok_or("no tbsCertificate")?;
    let sig_alg = outer.get(1).ok_or("no signature algorithm")?;
    let fields = children(tbs.body)?;
    let mut i = 0;
    if fields.first().is_some_and(|f| f.tag == 0xa0) {
        i = 1;
    }
    let get = |n: usize| fields.get(i + n).copied().ok_or_else(|| format!("tbsCertificate field {n} missing"));
    let serial = get(0)?;
    let issuer = get(2)?;
    let validity = children(get(3)?.body)?;
    let subject = get(4)?;
    let spki = children(get(5)?.body)?;
    let (subject, subject_org) = name_parts(subject)?;
    let (issuer, issuer_org) = name_parts(issuer)?;
    let mut key_algorithm = algorithm(*spki.first().ok_or("no key algorithm")?)?;
    // RSA: the modulus size.
    if key_algorithm == "RSA" {
        if let Some(bits) = spki.get(1).filter(|b| b.tag == 0x03).and_then(|b| b.body.get(1..)) {
            if let Ok((rsa, _)) = tlv(bits) {
                if let Ok(parts) = children(rsa.body) {
                    if let Some(m) = parts.first() {
                        let len = m.body.iter().position(|&b| b != 0).map_or(0, |z| m.body.len() - z);
                        key_algorithm = format!("RSA {} bits", len * 8);
                    }
                }
            }
        }
    }
    let exts = fields.iter().skip(i + 6).find(|f| f.tag == 0xa3).map(|e| sans(*e)).transpose()?.unwrap_or_default();
    let mut hash = Sha256::new();
    hash.update(der);
    let sha256 = hash.finalize().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
    Ok(CertSummary {
        subject,
        subject_org,
        issuer,
        issuer_org,
        not_before: time(*validity.first().ok_or("no notBefore")?)?,
        not_after: time(*validity.get(1).ok_or("no notAfter")?)?,
        sans: exts,
        key_algorithm,
        signature_algorithm: algorithm(*sig_alg)?,
        sha256,
        serial: serial.body.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAF: &[u8] = include_bytes!("../tests/fixtures/certs/leaf.der");
    const INTER: &[u8] = include_bytes!("../tests/fixtures/certs/intermediate.der");
    const ROOT: &[u8] = include_bytes!("../tests/fixtures/certs/root.der");
    const EXPIRED: &[u8] = include_bytes!("../tests/fixtures/certs/expired.der");
    const SELF: &[u8] = include_bytes!("../tests/fixtures/certs/selfsigned.der");

    #[test]
    fn leaf_certificate_fields() {
        let c = parse_der(LEAF).unwrap();
        assert_eq!((c.subject.as_str(), c.subject_org.as_str()), ("test.example", "Example Site"));
        assert_eq!((c.issuer.as_str(), c.issuer_org.as_str()), ("Krunk Test Intermediate", "Krunk Tests"));
        assert_eq!(c.sans, vec!["test.example", "*.test.example", "192.168.1.5"]);
        assert_eq!(c.not_before, 1_735_689_600, "2025-01-01T00:00:00Z");
        assert_eq!(c.not_after, 1_798_761_600, "2027-01-01T00:00:00Z");
        assert_eq!(c.key_algorithm, "RSA 2048 bits");
        assert_eq!(c.signature_algorithm, "SHA-256 with RSA");
        assert_eq!(c.serial, "03");
        assert_eq!(
            c.sha256,
            "4D:EC:3F:94:A7:09:6E:6D:7F:22:2D:78:D1:60:96:04:0E:96:8B:C7:FE:C9:67:79:FD:AC:AE:C2:69:E1:59:9F"
        );
    }

    #[test]
    fn other_certificates() {
        let inter = parse_der(INTER).unwrap();
        assert_eq!(inter.subject, "Krunk Test Intermediate");
        assert_eq!(inter.issuer, "Krunk Test Root");
        assert_eq!(inter.signature_algorithm, "ECDSA with SHA-256");
        let root = parse_der(ROOT).unwrap();
        assert_eq!((root.subject.as_str(), root.issuer.as_str()), ("Krunk Test Root", "Krunk Test Root"));
        assert_eq!(root.key_algorithm, "EC P-256");
        assert!(root.sans.is_empty());
        let exp = parse_der(EXPIRED).unwrap();
        assert_eq!((exp.not_before, exp.not_after), (1_577_836_800, 1_609_459_200));
        assert_eq!(exp.key_algorithm, "EC P-384");
        let s = parse_der(SELF).unwrap();
        assert_eq!((s.subject.as_str(), s.issuer.as_str(), s.subject_org.as_str()), ("localhost", "localhost", ""));
        assert_eq!(s.sans, vec!["localhost"]);
    }

    #[test]
    fn bad_der_is_refused_not_panicked() {
        assert!(parse_der(&[]).is_err());
        assert!(parse_der(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(parse_der(&LEAF[..100]).is_err());
        assert!(parse_der(&vec![0u8; MAX_DER + 1]).is_err());
        // Every truncation of a real certificate fails cleanly.
        for n in (0..LEAF.len()).step_by(37) {
            let _ = parse_der(&LEAF[..n]);
        }
        let mut flipped = LEAF.to_vec();
        for b in flipped.iter_mut().skip(10).step_by(53) {
            *b ^= 0xff;
        }
        let _ = parse_der(&flipped);
    }

    #[test]
    fn site_info_states() {
        let internal = site_info("https://krunk.invalid/settings.html", None);
        assert_eq!(internal.kind, SiteKind::Internal);
        let http = site_info("http://neverssl.com/", None);
        assert_eq!((http.kind, http.secure, http.certs.len()), (SiteKind::Http, false, 0));
        assert_eq!(site_info("file:///C:/x.html", None).kind, SiteKind::File);
        let ok = SslReport {
            url: String::new(),
            secure: true,
            cert_status: 0,
            version: 6,
            content_status: 0,
            chain: vec![LEAF.to_vec(), INTER.to_vec()],
        };
        let s = site_info("https://test.example/a", Some(&ok));
        assert!(s.secure && s.warnings.is_empty());
        assert_eq!((s.host.as_str(), s.protocol.as_deref(), s.cipher), ("test.example", Some("TLS 1.3"), None));
        assert_eq!(s.certs.len(), 2);
        assert_eq!(s.certs[1].subject, "Krunk Test Intermediate");
        // An expired, self-signed site: both errors named, nothing hidden.
        let bad = SslReport {
            url: "https://expired.badssl.com/".into(),
            secure: false,
            cert_status: 2 | 4,
            version: 5,
            content_status: 0,
            chain: vec![EXPIRED.to_vec()],
        };
        let b = site_info("https://example.com/", Some(&bad));
        assert!(!b.secure);
        assert_eq!(b.host, "expired.badssl.com", "the entry's URL wins over the tab's");
        assert_eq!(b.warnings.len(), 2, "{:?}", b.warnings);
        assert!(b.warnings[0].contains("expired") && b.warnings[1].contains("not trusted"));
        assert_eq!(b.certs.len(), 1);
        // Mixed content.
        let mixed = SslReport {
            url: String::new(),
            secure: true,
            cert_status: 0,
            version: 5,
            content_status: 1,
            chain: vec![LEAF.to_vec()],
        };
        assert!(site_info("https://mixed.badssl.com/", Some(&mixed)).warnings[0].contains("images or media"));
        let ran = SslReport { content_status: 2, ..mixed.clone() };
        assert!(site_info("https://mixed.badssl.com/", Some(&ran)).warnings[0].contains("scripts or styles"));
        // Informational bits are not errors.
        assert!(cert_status_names(65536 | 131072).is_empty());
        assert_eq!(ssl_version_name(42), None);
    }
}
