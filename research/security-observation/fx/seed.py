#!/usr/bin/env python3
"""E1 repetition seeds (DESIGN.md 12.3 "反復seed"): write each contract with its canaries changed per seed.

usage: seed.py <contract-dir> <out-dir> <seed>...      writes <out-dir>/s<seed>/F*.json
Every canary (the contract's canaries, and F12's TOKEN-F12-* strings in its files) keeps its length and
prefix; its last 4 characters become hex from sha256("<seed>:<canary>"). The replacement is applied to
the whole contract text, so files, accepted bodies and the canary table stay consistent. Lengths are
kept because the fixtures have fixed-size fields (F12). Nothing else (resource names, requests) changes.
"""
import hashlib
import json
import re
import sys
from pathlib import Path


def canaries(c):
    out = set(c.get("canaries", {}).values())
    for text in c["files"].values():
        out |= set(re.findall(r"TOKEN-F12-[0-9a-f]+", text))
    return sorted(out)


def main(src, dst, *seeds):
    for seed in seeds:
        d = Path(dst) / f"s{seed}"
        d.mkdir(parents=True, exist_ok=True)
        for f in sorted(Path(src).glob("F*.json")):
            text = f.read_text()
            c = json.loads(text)
            for k in canaries(c):
                new = k[:-4] + hashlib.sha256(f"{seed}:{k}".encode()).hexdigest()[:4]
                assert text.count(k) >= 1 and new not in text
                text = text.replace(k, new)
            json.loads(text)
            (d / f.name).write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
