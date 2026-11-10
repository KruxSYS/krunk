# Krunk VPN spike: extra env for cross-building openvpn-connect-sys with cargo-xwin.
# Source AFTER scripts/xwin-env.sh. Setting CXXFLAGS_<target> replaces cargo-xwin's own value,
# so this repeats cargo-xwin's flags (target, xwin CRT/SDK include dirs, /EHsc) and adds the
# tap-windows.h stand-in that OpenVPN 3 Core's Windows headers include.
spike=${spike:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
xw="${XWIN_CACHE_DIR:?source scripts/xwin-env.sh first}/xwin"
base="--target=x86_64-pc-windows-msvc -Wno-unused-command-line-argument -fuse-ld=lld-link"
base="$base /imsvc $xw/crt/include /imsvc $xw/sdk/include/ucrt /imsvc $xw/sdk/include/um"
base="$base /imsvc $xw/sdk/include/shared /imsvc $xw/sdk/include/winrt"
ovpn_cxxflags="$base /EHsc /imsvc $spike/win-include"
# cargo-xwin sets the hyphenated name, which cc-rs reads first; bash cannot export it, so wrap
# the cargo call:  env "CXXFLAGS_x86_64-pc-windows-msvc=$ovpn_cxxflags" cargo xwin build ...
unset CXXFLAGS_x86_64_pc_windows_msvc
