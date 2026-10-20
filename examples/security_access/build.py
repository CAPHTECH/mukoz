#!/usr/bin/env python3
"""Rebuild fixtures from assembly with GCC, an AArch64 assembler and llvm-objcopy."""
import argparse
import glob
import json
import hashlib
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--aarch64-as', default='aarch64-linux-gnu-as')
parser.add_argument('--directory', type=Path, default=HERE, help='assembly fixture directory with x86_64/ and aarch64/')
args = parser.parse_args()
HERE = args.directory.resolve()
objcopy = glob.glob(str(Path.home() / '.rustup/toolchains/*/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy'))[0]
manifest = {'objcopy': subprocess.check_output([objcopy, '--version'], text=True).splitlines()[0], 'artifacts': {}}
for isa in ('x86_64', 'aarch64'):
    for src in sorted((HERE / isa).glob('*.s')):
        with tempfile.TemporaryDirectory() as tmp:
            obj = Path(tmp) / 'probe.o'
            if isa == 'x86_64':
                cmd = ['gcc', '-c', '-o', str(obj), str(src)]
            else:
                cmd = [args.aarch64_as, '-o', str(obj), str(src)]
            subprocess.run(cmd, check=True)
            target = src.with_suffix('.bin')
            subprocess.run([objcopy, '-O', 'binary', '-j', '.text', str(obj), str(target)], check=True)
            manifest['artifacts'][str(target.relative_to(HERE))] = hashlib.sha256(target.read_bytes()).hexdigest()
(HERE / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
