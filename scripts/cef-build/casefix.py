#!/usr/bin/env python3
"""Case fixes for Chromium's Windows toolchain on a case-sensitive file system (no FUSE/ciopfs).

Chromium normally mounts the extracted toolchain through ciopfs; this host has no FUSE, so:
- headers: a clang VFS overlay (`vfs-include.yaml`, `case-sensitive: false`) that lists every
  file of the MSVC and Windows SDK include trees, so `#include <Windows.h>` resolves whatever
  the case (the SDK ships `Windows.h`, Chromium writes `windows.h`, and so on);
- libraries: a second overlay (`vfs-lib.yaml`) over the MSVC and SDK library trees for
  lld-link's `/vfsoverlay:` option, plus a mirror directory per architecture
  (`<out>/lib/<arch>/`) of lowercase symlinks to every library, searched after the real
  directories (`/libpath:`). Nothing is written inside the toolchain directory: Chromium hashes
  its file list and would re-extract the package (the same idea as LLVM's WinMsvc.cmake and
  xwin, which lowercase the library names);
- the toolchain's `clang-cl` and `lld-link` entries (symlinks to `clang` and `lld` in
  llvm-build/bin) are replaced in place by tiny wrappers that add the overlays. In place,
  because Chromium's `build/toolchain/win/rc/rc.py` hard-codes
  `third_party/llvm-build/Release+Asserts/bin/clang-cl`; the real binaries stay untouched
  and the wrappers call them by their real names (`clang --driver-mode=cl`, `lld -flavor link`).

    casefix.py <toolchain root (vs_files/<hash>)> <llvm-build root (Release+Asserts)> <output dir>

Idempotent: running it again rewrites the overlays and leaves existing wrappers and links.
"""
import os
import stat
import sys


def tree(root):
    """A redirecting-overlay entry for `root` and everything below it."""
    entries = []
    with os.scandir(root) as it:
        for e in sorted(it, key=lambda e: e.name):
            if e.is_dir(follow_symlinks=False):
                entries.append(tree(e.path))
            elif e.is_file(follow_symlinks=True) and not e.is_symlink():
                entries.append({"name": e.name, "type": "file", "external-contents": e.path})
    return {"name": os.path.basename(root), "type": "directory", "contents": entries}


def yaml_entry(entry, indent):
    pad = "  " * indent
    name = entry["name"].replace("\\", "\\\\").replace("'", "''")
    if entry["type"] == "file":
        ext = entry["external-contents"].replace("\\", "\\\\").replace("'", "''")
        return [f"{pad}{{ name: '{name}', type: file, external-contents: '{ext}' }}"]
    lines = [f"{pad}{{ name: '{name}', type: directory, contents: ["]
    for c in entry["contents"]:
        for l in yaml_entry(c, indent + 1):
            lines.append(l + ",")
    lines.append(f"{pad}]}}")
    return lines


def write_overlay(dirs, out):
    roots = []
    for d in dirs:
        e = tree(d)
        e["name"] = d  # an absolute root name
        roots.append(e)
    lines = ["{ version: 0, case-sensitive: 'false', overlay-relative: false, roots: ["]
    for r in roots:
        for l in yaml_entry(r, 1):
            lines.append(l + ",")
    lines.append("]}")
    with open(out, "w") as f:
        f.write("\n".join(lines) + "\n")


def lowercase_mirror(lib_dirs, out):
    """<out>/lib/<arch>/<lowercase name> -> the real library, for every arch directory found."""
    made = 0
    arch_dirs = {}
    for d in lib_dirs:
        for dirpath, _, files in os.walk(d):
            arch = os.path.basename(dirpath).lower()
            if arch not in ("x64", "x86", "arm64", "arm") or not files:
                continue
            target = os.path.join(out, "lib", arch)
            os.makedirs(target, exist_ok=True)
            arch_dirs.setdefault(arch, target)
            for name in files:
                if not name.lower().endswith(".lib"):
                    continue
                link = os.path.join(target, name.lower())
                if not os.path.lexists(link):
                    os.symlink(os.path.join(dirpath, name), link)
                    made += 1
    return made, arch_dirs


def install_wrapper(bin_dir, name, real, extra, pre=""):
    """Replace bin/<name> (a symlink to the real tool) by a wrapper script; idempotent."""
    path = os.path.join(bin_dir, name)
    if os.path.islink(path) or not os.path.exists(path):
        if os.path.lexists(path):
            os.unlink(path)
    elif os.path.isfile(path):
        with open(path, "rb") as f:
            head = f.read(2)
        if head != b"#!":
            raise SystemExit(f"{path} is a regular binary, not a symlink: refusing to replace it")
    with open(path, "w") as f:
        f.write("#!/bin/sh\n# casefix.py wrapper (Krunk CEF cross build); see scripts/cef-build/README.md\n")
        f.write(pre + 'exec \'%s\' %s "$@"\n' % (os.path.join(bin_dir, real), extra))
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def main():
    root, llvm, out = (os.path.abspath(a) for a in sys.argv[1:4])
    os.makedirs(out, exist_ok=True)
    msvc = [os.path.join(root, "VC", "Tools", "MSVC", v) for v in os.listdir(os.path.join(root, "VC", "Tools", "MSVC"))]
    kits = os.path.join(root, "Windows Kits", "10")
    sdk_versions = os.listdir(os.path.join(kits, "Include"))
    include_dirs = []
    lib_dirs = []
    for m in msvc:
        include_dirs += [os.path.join(m, "include"), os.path.join(m, "atlmfc", "include")]
        lib_dirs += [os.path.join(m, "lib"), os.path.join(m, "atlmfc", "lib")]
    for v in sdk_versions:
        include_dirs.append(os.path.join(kits, "Include", v))
        lib_dirs.append(os.path.join(kits, "Lib", v))
    include_dirs = [d for d in include_dirs if os.path.isdir(d)]
    lib_dirs = [d for d in lib_dirs if os.path.isdir(d)]
    # VS 2026 names the host compiler directories `Hostx64`/`Hostx86`; Chromium's setup_toolchain.py
    # and SetEnv.x64.json say `HostX64`. A directory symlink does not change the toolchain's file
    # list (os.walk does not follow it), so Chromium's package hash check stays happy.
    host_links = 0
    for m in msvc:
        bin_dir_msvc = os.path.join(m, "bin")
        for want in ("HostX64", "HostX86", "HostARM64"):
            have = [d for d in os.listdir(bin_dir_msvc) if d.lower() == want.lower()]
            if have and want not in have and not os.path.lexists(os.path.join(bin_dir_msvc, want)):
                os.symlink(have[0], os.path.join(bin_dir_msvc, want))
                host_links += 1
    vfs_inc = os.path.join(out, "vfs-include.yaml")
    vfs_lib = os.path.join(out, "vfs-lib.yaml")
    links, arch_dirs = lowercase_mirror(lib_dirs, out)
    write_overlay(include_dirs, vfs_inc)
    write_overlay(lib_dirs, vfs_lib)
    bin_dir = os.path.join(llvm, "bin")
    for real in ("clang", "lld"):
        if not os.path.isfile(os.path.join(bin_dir, real)):
            raise SystemExit(f"missing {bin_dir}/{real}")
    # clang-cl: the overlay goes to cc1 (-Xclang) so it works for compiling and for rc.py's /P.
    install_wrapper(bin_dir, "clang-cl", "clang",
                    "--driver-mode=cl -Xclang -ivfsoverlay -Xclang '%s'" % vfs_inc)
    # The mirror directory of the target architecture comes from /MACHINE: (Chromium always
    # passes it); without it, the x64 mirror is used.
    # `lld-link /lib` (the lib.exe replacement) needs /lib as the first argument and takes no
    # overlay or search path: pass it straight through.
    lld_real = os.path.join(bin_dir, "lld")
    mirror_sh = ("case \"$1\" in /lib|/LIB) exec '%s' -flavor link \"$@\";; esac; " % lld_real +
                 "arch=x64; for a in \"$@\"; do case \"$a\" in /MACHINE:*|/machine:*) "
                 "arch=$(printf %s \"${a#*:}\" | tr A-Z a-z);; esac; done; ")
    # /ignore:4099: MSVC 14.51's desktop libcmt.lib contains objects whose debug records point at
    # lib/onecore/x64/libcmt.amd64.onecore.pdb (not next to the library), so lld-link warns
    # LNK4099 "Cannot use debug info", which Chromium's /WX turns into an error. The CRT's debug
    # info is of no use here (symbol_level=0, no PDBs shipped).
    install_wrapper(bin_dir, "lld-link", "lld",
                    "-flavor link /ignore:4099 '/vfsoverlay:%s' \"/libpath:%s/lib/$arch\"" % (vfs_lib, out),
                    pre=mirror_sh)
    print("overlays: %s (%d include trees), %s (%d lib trees); %d lowercase library links in %s; "
          "%d Host* directory links; wrappers in %s"
          % (vfs_inc, len(include_dirs), vfs_lib, len(lib_dirs), links, sorted(arch_dirs), host_links, bin_dir))


if __name__ == "__main__":
    main()
