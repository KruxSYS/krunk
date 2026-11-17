//! Krunk's in-browser VPN (the `vpn` crate) inside `libkrunk_ffi.so`.
//!
//! The library links `vpn` without its `openvpn` feature (WireGuard only): an OpenVPN file is
//! recognised and refused with `OPENVPN_UNSUPPORTED`. The full `KrunkVpn` object comes later; for
//! now only the import check is exported.

/// The kind of an imported VPN profile file (`"wireguard"` or `"openvpn"`), or the reason it was
/// refused, as `"error: …"` (never quotes the file's secrets).
#[uniffi::export]
pub fn vpn_import_kind(file_name: String, text: String) -> String {
    match krunk_vpn::import(&file_name, &text) {
        Ok((_, info)) => info.kind.as_str().to_owned(),
        Err(e) => format!("error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_wireguard_refuses_openvpn() {
        let wg = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.0.0.2/32\n\
                  [Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nEndpoint = 192.0.2.1:51820\nAllowedIPs = 0.0.0.0/0\n";
        assert_eq!(vpn_import_kind("a.conf".into(), wg.into()), "wireguard");
        let ovpn = vpn_import_kind("b.ovpn".into(), "client\nremote vpn.example 1194\n".into());
        assert!(ovpn.starts_with("error: "), "{ovpn}");
    }
}
