#!/usr/bin/env python3
"""Restore the archived F11 and regression records before running recorded-data tests.
Usage: python3 restore_evidence.py [destination] (default: this directory).
Only restores files covered by the checked-in SHA256 manifest. Python 3.12+ required.
"""
import hashlib
import json
import sys
import tarfile
from pathlib import Path


def main():
    here = Path(__file__).resolve().parent
    dest = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else here
    evidence = here / 'evidence'
    manifest = json.loads((evidence / 'manifest.json').read_text())
    for name, entry in manifest.items():
        path = evidence / name
        if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError(f'archive digest mismatch: {name}')
    dest.mkdir(parents=True, exist_ok=True)
    for name in manifest:
        with tarfile.open(evidence / name) as archive:
            archive.extractall(dest, filter='data')
        print(f'restored {name} into {dest}')


if __name__ == '__main__':
    main()
