//! QR codes as SVG, rendered on the server so the pages need no QR library (TOTP setup, the
//! sign-in link of `link/authorize`, and "Add a device").

/// An SVG document of `data`, at least 200×200 with the quiet zone; `None` if it doesn't fit.
pub fn svg(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    Some(code.render::<qrcode::render::svg::Color>().min_dimensions(200, 200).quiet_zone(true).build())
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_a_link() {
        let s = super::svg("https://krunk.kruxsys.com/pair#AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
        assert!(s.contains("<svg") && s.ends_with("</svg>"), "{s}");
        assert!(!s.contains("<script"));
    }
}
