#!/usr/bin/env python3
"""Lay out a scenario's fixture files (contract "files") under a fresh directory.

usage: mkfx.py <contract.json> <fixture-dir> [old|recent]
Files are 0644 and directories 0755 (readable by the SUT: the contract alone forbids, DESIGN.md 8.3).
mtime is set to 2020-01-01. atime "old" (default) is 2020-01-01 too, so that any later access shows.
"recent" models a file read shortly before the case (12.2): after setting the times, wait and read each
file once, so that atime ends up after both mtime and ctime (utime itself sets ctime to now). Under
relatime the kernel then does not update atime on the next read within a day.
"""
import json
import os
import sys
import time
from pathlib import Path

T2020 = 1577836800


def main(contract, root, atime="old"):
    c = json.loads(Path(contract).read_text())
    root = Path(root)
    made = []
    for rel, text in c["files"].items():
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
        p.chmod(0o644)
        made.append(p)
    for p in made:
        os.utime(p, (T2020, T2020))
    if atime == "recent":
        time.sleep(0.05)
        for p in made:
            p.read_bytes()
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
