#!/usr/bin/env python3
"""No VM or network: run the F11 handler via stdio, preserving its allocation/access code.
Requires gcc, objdump, Valgrind and GDB. Own child processes only. Output must be new.
Example: python3 f11_local.py runs/f11-local-1 --valgrind /path/valgrind --gdb /path/gdb
Set VALGRIND_LIB, LD_LIBRARY_PATH, PYTHONHOME as needed for unpacked tools.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

import c_check as C

HERE = Path(__file__).resolve().parent
PROP = 'external.mem.heap.addressable'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def claims(c, case, text, mode):
    b = C.binding_of(c)
    br = json.dumps(b).encode()
    if mode == 'c':
        events, receipt = C.M.parse(text)
        # Each sequential case owns a newly launched process session, not a VM namespace.
        for e in events:
            e['attribution']['case'] = 'exclusive_process_group'
    else:
        events = []
        receipt = dict(start='before_sut_exec', end='drained', gaps=[], lost_events=0, capabilities=[])
    obs = dict(schema='secobs.observations/0', binding_sha256=hashlib.sha256(br).hexdigest(),
               cases=[dict(case_id=case['id'], principal=case['principal'], requests_sent=1,
                           receipt=receipt, events=events)])
    out, _ = C.A.convert(br, b, json.dumps(obs).encode(), obs, '0' * 64, 'self')
    return obs, out


def response_ok(c, case, raw):
    header, body = raw.split(b'\r\n\r\n', 1)
    status = int(header.split(b' ', 2)[1])
    length = int(re.search(rb'Content-Length: (\d+)', header).group(1))
    return length == len(body) and any(
        status == alt['status'] and ('file' not in alt or
        body == (c['files'][alt['file']] + alt.get('suffix', '')).encode())
        for alt in case['accept'][0])


def run(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    c = json.loads((HERE / 'contract/F11.json').read_text())
    fixture = out / 'fixture'
    for name, text in c['files'].items():
        p = fixture / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    build = out / 'build'
    build.mkdir()
    env = {'compiler': subprocess.check_output(['gcc', '--version'], text=True).splitlines()[0],
           'valgrind': subprocess.check_output([args.valgrind, '--version'], text=True).strip(),
           'gdb': subprocess.check_output([args.gdb, '--version'], text=True).splitlines()[0],
           'kernel': os.uname().release, 'contract_sha256': sha(HERE / 'contract/F11.json'),
           'source_sha256': sha(HERE / 'src/f11.c'), 'runner_sha256': sha(__file__),
           'normalizer_sha256': sha(HERE.parent / 'normalize/memcheck.py'),
           'adapter_sha256': sha(HERE.parent / 'adapter/adapter.py'),
           'http_header_sha256': sha(HERE / 'src/http.h'), 'gdb_script_sha256': sha(HERE / 'gdb/F11.gdb'),
           'artifacts': {}, 'build_commands': [], 'repetitions': args.repetitions,
           'scope': 'stdio handler calibration; no OS collector, HTTP transport or VM comparison'}
    for v in 'VRH':
        binary = build / f'f11-{v}'
        cmd = ['gcc', '-O2', '-g', '-Wall', '-Wextra', '-Wno-unused-function', '-DF11_STDIO',
               f'-DVARIANT_{v}', '-o', str(binary), str(HERE / 'src/f11.c')]
        subprocess.run(cmd, check=True)
        env['build_commands'].append(cmd)
        env['artifacts'][v] = sha(binary)
        (build / f'{v}.asm').write_bytes(subprocess.check_output(['objdump', '-d', str(binary)]))
    (out / 'env.json').write_text(json.dumps(env, indent=2) + '\n')
    tally, problems = Counter(), []
    for rep in range(1, args.repetitions + 1):
        for v in 'VRH':
            binary = build / f'f11-{v}'
            for case in c['cases']:
                want = bool(case['labels'][v])
                base = [str(binary), str(fixture), case['requests'][0]['target']]
                for mode in ('native', 'gdb', 'c0', 'c'):
                    d = out / f'r{rep}' / mode / v / case['id']
                    d.mkdir(parents=True)
                    if sha(binary) != env['artifacts'][v]:
                        raise RuntimeError('artifact changed during execution')
                    cmd = base
                    if mode in ('c0', 'c'):
                        cmd = [args.valgrind, '--tool=' + ('none' if mode == 'c0' else 'memcheck')]
                        if args.debug:
                            cmd += [f'--extra-debuginfo-path={args.debug}']
                        cmd += (['--log-file=' + str(d / 'vg.log')] if mode == 'c0' else
                                ['--xml=yes', '--xml-file=' + str(d / 'vg.xml'), '--error-limit=no', '--leak-check=no'])
                        cmd += base
                    elif mode == 'gdb':
                        cmd = [args.gdb, '-nx', '-batch', '-ex', 'set disable-randomization off',
                               '-ex', 'set debuginfod enabled off', '-ex', 'tbreak handle', '-ex', 'run',
                               '-x', str(HERE / 'gdb/F11.gdb'), '--args'] + base
                    (d / 'command.json').write_text(json.dumps(cmd) + '\n')
                    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       timeout=20, start_new_session=True)
                    (d / 'stdout').write_bytes(p.stdout)
                    (d / 'stderr').write_bytes(p.stderr)
                    (d / 'exit.txt').write_text(str(p.returncode) + '\n')
                    key = f'r{rep}/{mode}/{v}/{case["id"]}'
                    errors = []
                    if p.returncode:
                        errors.append(f'exit={p.returncode}')
                    if mode == 'gdb':
                        g = p.stdout.decode(errors='replace')
                        armed = len(set(re.findall(r'^Hardware access \(read/write\) watchpoint (\d+): -location', g, re.M)))
                        if armed != 4 or 'WATCHING' not in g or 'exited normally' not in g or bool('WITNESS_HIT' in g) != want:
                            errors.append('missing/incorrect native witness')
                        tally['gdb/violations'] += int('WITNESS_HIT' in g)
                    else:
                        try:
                            ok = response_ok(c, case, p.stdout)
                        except (ValueError, AttributeError, IndexError):
                            ok = False
                        if not ok or p.stderr:
                            errors.append('response mismatch or stderr not empty')
                        tally[f'{mode}/responses_ok'] += int(ok)
                        obs, result = claims(c, case, (d / 'vg.xml').read_text() if mode == 'c' else '', mode)
                        (d / 'observations.json').write_text(json.dumps(obs, indent=2) + '\n')
                        (d / 'claims.json').write_text(json.dumps(result, indent=2) + '\n')
                        actual = next(x['eval'] for x in result['claims'] if x['property'] == PROP)
                        expected = (C.A.VIO if want else C.A.SAT) if mode == 'c' else C.A.NOT
                        if actual != expected:
                            errors.append(f'{actual} != {expected}')
                        tally[f'{mode}/{actual}'] += 1
                        if mode == 'c':
                            xml = ET.fromstring((d / 'vg.xml').read_text())
                            es = xml.findall('error')
                            freed = [e for e in es if e.findtext('kind') == 'InvalidRead'
                                     and "free'd" in (e.findtext('auxwhat') or '')
                                     and len(e.findall('stack')) >= 3
                                     and any(f.findtext('file') == 'f11.c' for f in e.findall('stack/frame'))]
                            if bool(freed) != want or len(es) != len(freed):
                                errors.append('unexpected/missing freed-block read with allocation/free stacks')
                    tally[f'{mode}/runs'] += 1
                    problems += [key + ': ' + e for e in errors]
        print(f'repetition {rep}: {sum(tally[k] for k in tally if k.endswith("/runs"))} runs, {len(problems)} problems', flush=True)
    result = {'schema': 'secobs.f11_local/0', 'tally': dict(tally), 'problems': problems}
    (out / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    return bool(problems)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('out', type=Path)
    p.add_argument('--valgrind', default='valgrind')
    p.add_argument('--gdb', default='gdb')
    p.add_argument('--debug', type=Path)
    p.add_argument('--repetitions', type=int, default=3)
    args = p.parse_args()
    if args.repetitions < 1:
        p.error('repetitions must be positive')
    sys.exit(run(args))
