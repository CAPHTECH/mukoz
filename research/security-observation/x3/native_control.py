#!/usr/bin/env python3
"""Optional x86-64 host control: Memcheck on the same assembly in one live allocation.
This is not a native Mukoz memory verdict. It compares allocator addressability with caller policy.
"""
import argparse
import hashlib
import json
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('out', type=Path)
    p.add_argument('--valgrind', default='valgrind')
    p.add_argument('--debug', type=Path)
    a = p.parse_args()
    out = a.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    rows, problems = [], []
    for name in ('correct', 'read_audit', 'restore_source', 'read_past_source', 'scratch'):
        assembly = ROOT / 'examples/security_access/x86_64' / f'{name}.s'
        exe = out / name
        subprocess.run(['gcc', '-O2', '-g', '-Wall', '-Wextra', '-o', str(exe),
                        str(HERE / 'native_control.c'), str(assembly)], check=True)
        for value in (0, 1, (1 << 64) - 1):
            dest = out / f'{name}-{value}'
            dest.mkdir()
            cmd = [a.valgrind, '--tool=memcheck', '--error-limit=no', '--leak-check=no', '--xml=yes',
                   '--xml-file=' + str(dest / 'vg.xml')]
            if a.debug:
                cmd += ['--extra-debuginfo-path=' + str(a.debug)]
            cmd += [str(exe), str(value)]
            run = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
            (dest / 'command.json').write_text(json.dumps(cmd) + '\n')
            (dest / 'stdout').write_text(run.stdout)
            (dest / 'stderr').write_text(run.stderr)
            expected = f'0 {value:016x} {value:016x} 4c544e4544455243\n'
            xml = ET.parse(dest / 'vg.xml')
            errors = xml.findall('error')
            finished = [x.text for x in xml.findall('status/state')][-1:] == ['FINISHED']
            if run.returncode or run.stderr or run.stdout != expected or errors or not finished:
                problems.append(f'{name}/{value}: functional/tool/diagnostic mismatch')
            rows.append({'variant': name, 'value': hex(value), 'response_ok': run.stdout == expected,
                         'memcheck_errors': len(errors), 'exit': run.returncode,
                         'assembly_sha256': hashlib.sha256(assembly.read_bytes()).hexdigest()})
    report = {'schema': 'secobs.x3.native_control/0', 'scope': 'x86-64 only, 3 inputs per variant, one live allocation',
              'valgrind': subprocess.check_output([a.valgrind, '--version'], text=True).strip(),
              'rows': rows, 'problems': problems}
    (out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'{len(rows)} runs, {sum(r["memcheck_errors"] for r in rows)} Memcheck errors, {len(problems)} problems')
    return bool(problems)


if __name__ == '__main__':
    sys.exit(main())
