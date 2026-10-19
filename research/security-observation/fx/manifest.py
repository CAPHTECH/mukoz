#!/usr/bin/env python3
"""A1 file manifest of the fixture (DESIGN.md 9.1): size, mode, atime/mtime/ctime (ns) per file,
the mount options, and (only with --hash) the content sha256.

usage: manifest.py <fixture-dir> <out.json> [--hash]
stat() does not touch atime. --hash reads the files, so it is used only after the run, after the
stat pass and after the ground-truth layers have stopped.
"""
import hashlib
import json
import os
import sys
from pathlib import Path


def mount_options(root):
    best = None
    for ln in Path("/proc/self/mountinfo").read_text().splitlines():
        f = ln.split()
        if root == f[4] or root.startswith(f[4].rstrip("/") + "/"):
            if best is None or len(f[4]) > len(best[4]):
                best = f
    return best[5] if best else None


def main(root, out, *flags):
    root = os.path.realpath(root)
    files = {}
    for dp, _, fs in os.walk(root):
        for f in fs:
            p = os.path.join(dp, f)
            st = os.lstat(p)
            files[os.path.relpath(p, root)] = {"size": st.st_size, "mode": oct(st.st_mode & 0o7777),
                                               "atime_ns": st.st_atime_ns, "mtime_ns": st.st_mtime_ns, "ctime_ns": st.st_ctime_ns}
    if "--hash" in flags:
        for rel, m in files.items():
            m["sha256"] = hashlib.sha256(Path(root, rel).read_bytes()).hexdigest()
    Path(out).write_text(json.dumps({"root": root, "mount_options": mount_options(root), "files": files}, indent=1, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
