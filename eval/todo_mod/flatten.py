"""Flatten a mukoz.link/1 file into one image starting at 0xf0000 (import table first, then
module i at 0x100000 + i * 0x100000), for ../todo/launch -b f0000.
usage: python3 flatten.py <link.toml> <out.img> [name=path ...]   (name=path overrides a module file)"""
import os, struct, sys, tomllib

link = sys.argv[1]
over = dict(a.split("=", 1) for a in sys.argv[3:])
L = tomllib.load(open(link, "rb"))
base = os.path.dirname(os.path.abspath(link))
mods = L["modules"]
addr = {}
img = bytearray()
for i, m in enumerate(mods):
    p = over.get(m["name"]) or os.path.join(base, m["path"])
    b = open(p, "rb").read()
    if len(b) > 0x100000:
        sys.exit(f"{m['name']}: more than 1 MiB")
    off = 0x10000 + i * 0x100000
    img += b"\0" * (off + len(b) - len(img)) if len(img) < off + len(b) else b""
    img[off:off + len(b)] = b
    for sym, o in m.get("exports", {}).items():
        addr[f"{m['name']}.{sym}"] = 0x100000 + i * 0x100000 + o
for k, target in L.get("imports", {}).items():
    struct.pack_into("<Q", img, 8 * int(k), addr[target])
open(sys.argv[2], "wb").write(img)
