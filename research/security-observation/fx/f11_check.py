#!/usr/bin/env python3
"""F11 calibration: require 24 cases in each of native/a/b/c0/c and identical artifact hashes.
Native uses GDB as an independent witness; a has no tracer. Every response must match,
including violating V cases. Memcheck XML is additionally checked for a freed-block InvalidRead.
Usage: f11_check.py <runs-prefix> (directories <prefix>-native, -a, -b, -c0, -c).
"""
import hashlib
import json
import re
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

import a1
import c_check as C
import witness as W

HERE = Path(__file__).resolve().parent
MEM = 'MEM.HEAP.ADDRESSABLE'


def check(prefix):
    c = json.loads((HERE / 'contract/F11.json').read_text())
    cw = dict(c, properties=[p for p in c['properties'] if not p.startswith('MEM.')])
    problems, tally, hashes = [], Counter(), []
    for mode in ('native', 'a', 'b', 'c0', 'c'):
        root = Path(f'{prefix}-{mode}')
        try:
            env = (root / 'env.txt').read_text()
            h = {name: digest for digest, name in re.findall(r'^([0-9a-f]{64})  (bin/f11-[vrh])$', env, re.M)}
            if len(h) != 3:
                problems.append(f'{mode}: missing artifact hashes')
            hashes.append(h)
            for v in 'VRH':
                for case in c['cases']:
                    d = root / 'F11' / v / case['id']
                    key = f'{mode}/{v}/{case["id"]}'
                    try:
                        seen, func, errs = W.judge(c if mode == 'native' else cw, case, v, d)
                        if errs or not func:
                            problems.append(f'{key}: FUNC={func}, {errs}')
                        want = set(case['labels'][v])
                        status = (d / 'sbx.status').read_text()
                        if (d / 'exit.txt').read_text().strip() != '143' or 'all_exited' not in status or 'grace_expired' in status:
                            problems.append(f'{key}: unexpected termination or incomplete drain')
                        if mode == 'native' and seen != want:
                            problems.append(f'{key}: witness {seen} != {want}')
                        for cond in ('A0', 'A1'):
                            got = a1.judge(c, case, d, cond)
                            if got[MEM][0] != a1.NOT or got['FUNC.ITEM_READ'][0] != a1.SAT:
                                problems.append(f'{key}: {cond} {got}')
                        for name in ('stdout.txt', 'stderr.txt'):
                            if (d / name).read_bytes():
                                problems.append(f'{key}: diagnostic in {name}')
                        if mode in ('b', 'c0', 'c'):
                            claims = C.claims(mode, c, case, d)
                            expected = (C.A.VIO if MEM in want else C.A.SAT) if mode == 'c' else C.A.NOT
                            actual = (C.A.NOT if not claims else 'UNEXPECTED_CLAIM') if mode == 'b' else claims.get(MEM, ('MISSING_CLAIM', None))[0]
                            if actual != expected:
                                problems.append(f'{key}: {actual} != {expected}')
                            tally[f'{mode}/{actual}'] += 1
                        if mode == 'c':
                            xml = ET.fromstring((d / 'vg.xml').read_text())
                            errors = xml.findall('error')
                            freed = [e for e in errors if e.findtext('kind') == 'InvalidRead'
                                     and 'free\'d' in (e.findtext('auxwhat') or '')
                                     and any(f.findtext('file') == 'f11.c' and f.findtext('fn') in ('cached_flag', 'handle')
                                             for f in e.findall('stack/frame'))
                                     and len(e.findall('stack')) >= 3]
                            if bool(freed) != (MEM in want) or len(errors) != len(freed):
                                problems.append(f'{key}: unexpected or missing freed-block diagnostic')
                            tally['freed_block_reads'] += len(freed)
                        tally[f'{mode}/responses_ok'] += int(func)
                    except (OSError, ValueError, KeyError, ET.ParseError) as exc:
                        problems.append(f'{key}: {exc}')
        except OSError as exc:
            problems.append(f'{mode}: {exc}')
    if len(hashes) != 5 or any(h != hashes[0] for h in hashes):
        problems.append('artifact hashes differ across conditions')
    return {'schema': 'secobs.f11_calibration/0', 'contract_sha256': hashlib.sha256((HERE / 'contract/F11.json').read_bytes()).hexdigest(),
            'tally': dict(sorted(tally.items())), 'artifact_hashes': hashes[0] if hashes else {}, 'problems': problems}


if __name__ == '__main__':
    result = check(sys.argv[1])
    print(json.dumps(result, ensure_ascii=False, indent=2))
    sys.exit(bool(result['problems']))
