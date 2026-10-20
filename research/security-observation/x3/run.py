#!/usr/bin/env python3
"""Compare final-state observations with Mukoz access contracts, without native execution or a VM.
The permissive protocol runs are controls, never Mukoz security admissions. The CLI checks keep
all required claims. Capture code/digests, protocol transcript, full CLI results and replay evidence.
"""
import argparse
import gzip
import hashlib
import json
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
EXAMPLE = ROOT / 'examples/security_access'
NAMES = ('correct', 'read_audit', 'restore_source', 'read_past_source', 'scratch')
CODE, DATA, STACK, SP, END = 0x100000, 0x200000, 0x300000, 0x300f00, 0xdeadd000


def digest(data):
    return hashlib.sha256(data).hexdigest()


class Engine:
    def __init__(self, path, log):
        self.p = subprocess.Popen([str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.log = log
        self.identity = self.call({'op': 'hello'})
        if self.identity.get('protocol') != 'mukoz-emu/1':
            raise RuntimeError('bad engine protocol')

    def call(self, obj):
        self.p.stdin.write(json.dumps(obj) + '\n')
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError('engine disconnected')
        result = json.loads(line)
        self.log.write(json.dumps({'request': obj, 'reply': result}) + '\n')
        return result

    def close(self):
        self.p.stdin.write('{"op":"quit"}\n')
        self.p.stdin.flush()
        self.p.wait(timeout=5)


def setup(isa, blob, value, strict):
    data = bytearray(4096)
    data[32:40] = value.to_bytes(8, 'little')
    data[64:72] = b'OLD-DATA'
    data[96:104] = b'CREDENTL'
    if isa == 'x86_64':
        regs = {r: 0 for r in ('rax rbx rcx rdx rsi rdi rbp rsp r8 r9 r10 r11 r12 r13 r14 r15 rflags').split()}
        regs.update(rsp=SP, rdi=DATA + 64, rsi=DATA + 32, rdx=DATA + 96, rflags=0x202)
        sentinel = [{'addr': SP, 'hex': END.to_bytes(8, 'little').hex()}]
        stack_hi = SP + 8
    else:
        regs = {f'x{i}': 0 for i in range(31)}
        regs.update(sp=SP, x0=DATA + 64, x1=DATA + 32, x2=DATA + 96, x30=END, nzcv=0)
        sentinel, stack_hi = [], SP
    allowed = [[CODE, CODE + len(blob), 1, 0], [STACK, stack_hi, 1, 1]]
    allowed += ([[DATA + 32, DATA + 40, 1, 0], [DATA + 64, DATA + 72, 1, 1],
                 [DATA + 96, DATA + 104, 0, 1]] if strict else [[DATA, DATA + 4096, 1, 1]])
    spec = {'op': 'run', 'isa': isa, 'map': [{'addr': CODE, 'size': 4096, 'perm': 'rwx'},
            {'addr': DATA, 'size': 4096, 'perm': 'rw'}, {'addr': STACK, 'size': 4096, 'perm': 'rw'}],
            'write': [{'addr': CODE, 'hex': blob.hex()}, {'addr': DATA, 'hex': data.hex()}] + sentinel,
            'regs': list(regs.items()), 'entry': CODE, 'until': END, 'insn_limit': 100, 'timeout_ms': 1000,
            'allowed': allowed, 'code': [[CODE, CODE + len(blob)]], 'frozen': {}, 'breaks': []}
    return spec, list(regs)


def execute(engine, isa, blob, value, strict):
    spec, regs = setup(isa, blob, value, strict)
    end = engine.call(spec)
    if end.get('event') != 'end':
        raise RuntimeError(f'unexpected engine event {end}')
    if end.get('error'):
        raise RuntimeError(f'engine error {end}')
    state = {'regs': engine.call({'op': 'reg_read', 'names': regs})['regs'],
             'data': engine.call({'op': 'mem_read', 'addr': DATA, 'len': 4096})['hex'],
             'stack': engine.call({'op': 'mem_read', 'addr': STACK, 'len': 4096})['hex']}
    if engine.call({'op': 'done'}).get('ok') is not True:
        raise RuntimeError('engine did not finish')
    return end, state


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('out', type=Path)
    p.add_argument('--mukoz', type=Path, default=ROOT / 'target/release/mukoz')
    p.add_argument('--engine', type=Path, default=ROOT / 'target/release/mukoz-emu-icicle')
    args = p.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    rng = random.Random(20261020)
    values = [0, 1, 0xff, 0xffff, 1 << 31, 1 << 32, 1 << 63, (1 << 64) - 1]
    values += [rng.getrandbits(64) for _ in range(16)]
    problems, rows, controls, cli = [], [], {}, {}
    with gzip.open(out / 'protocol.jsonl.gz', 'wt') as log:
        engine = Engine(args.engine.resolve(), log)
        try:
            for isa in ('x86_64', 'aarch64'):
                for value in values:
                    reference = None
                    for name in NAMES:
                        blob = (EXAMPLE / isa / f'{name}.bin').read_bytes()
                        end, final = execute(engine, isa, blob, value, False)
                        if end['stop'] is not None or end['pc'] != END:
                            raise RuntimeError('permissive control did not return')
                        if reference is None:
                            reference = final
                        equal = final == reference
                        if not equal:
                            problems.append(f'{isa}/{name}/{value}: final state differs')
                        checked, _ = execute(engine, isa, blob, value, True)
                        stop = checked.get('stop')
                        expected = name not in ('correct', 'scratch')
                        caught = bool(stop and stop.get('kind') == 'access')
                        access = {'read_audit': (False, DATA + 96), 'restore_source': (True, DATA + 32),
                                  'read_past_source': (False, DATA + 40)}.get(name)
                        exact_access = not expected or (caught and (stop.get('write'), stop.get('addr')) == access and stop.get('size') == 1)
                        if caught != expected or not exact_access or (not expected and checked['pc'] != END):
                            problems.append(f'{isa}/{name}/{value}: unexpected access check {checked}')
                        rows.append({'isa': isa, 'variant': name, 'value': hex(value), 'final_state_equal': equal,
                                     'final_state_sha256': digest(json.dumps(final, sort_keys=True).encode()),
                                     'access_stop': stop, 'artifact_sha256': digest(blob)})
                controls[isa] = {'cases': len(values), 'variants': len(NAMES),
                                 'all_final_states_equal': all(r['final_state_equal'] for r in rows if r['isa'] == isa)}
        finally:
            engine.close()
    (out / 'controls.json').write_text(json.dumps(rows, indent=2) + '\n')
    cli_env = dict(os.environ, MUKOZ_EMU=str(args.engine.resolve()))
    for isa in ('x86_64', 'aarch64'):
        for name in NAMES:
            dest = out / isa / name
            dest.mkdir(parents=True)
            cmd = [str(args.mukoz.resolve()), 'check', str(EXAMPLE / f'suite.{isa}.toml'),
                   '--artifact', str(EXAMPLE / isa / f'{name}.bin'), '--store', str(dest / 'store')]
            run = subprocess.run(cmd, capture_output=True, text=True, timeout=60, env=cli_env)
            (dest / 'check.json').write_text(run.stdout)
            result = json.loads(run.stdout)
            if not result.get('ok'):
                raise RuntimeError(f'Mukoz check failed: {result}')
            data = result['data']
            accepted = name in ('correct', 'scratch')
            expected = 'ACCEPT_WITHIN_SCOPE' if accepted else 'REJECT'
            if data['assessment']['admission'] != expected:
                problems.append(f'{isa}/{name}: CLI {data["assessment"]["admission"]} != {expected}')
            memory = next(c for c in data['claims'] if c['property'] == 'machine.memory.access')
            count_key = 'cases_satisfied' if accepted else 'cases_violated'
            if memory[count_key] != 24:
                problems.append(f'{isa}/{name}: not all planned cases have the expected memory verdict')
            if not accepted:
                finding = next((f for f in data['findings'] if f['property'] == 'machine.memory.access'), None)
                if not finding:
                    problems.append(f'{isa}/{name}: no access counterexample')
                else:
                    replay = subprocess.run([str(args.mukoz.resolve()), 'replay', finding['counterexample_id'],
                                              '--store', str(dest / 'store')], capture_output=True, text=True, timeout=60, env=cli_env)
                    (dest / 'replay.json').write_text(replay.stdout)
                    r = json.loads(replay.stdout)
                    # Replay results must retain the announced violated property.
                    if not r.get('ok') or not any(c.get('property') == 'machine.memory.access' and c.get('evaluation') == 'VIOLATED' for c in (r.get('data') or {}).get('claims', [])):
                        problems.append(f'{isa}/{name}: counterexample did not replay')
            cli[f'{isa}/{name}'] = data['assessment']['admission']
    summary = {'schema': 'secobs.x3/0', 'scope': 'synthetic routines, enumerated inputs, emulated only',
               'engine': engine.identity, 'engine_sha256': digest(args.engine.read_bytes()),
               'mukoz_sha256': digest(args.mukoz.read_bytes()), 'runner_sha256': digest(Path(__file__).read_bytes()),
               'controls': controls, 'protocol_runs': len(rows) * 2, 'cli': cli, 'problems': problems}
    (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))
    return bool(problems)


if __name__ == '__main__':
    sys.exit(main())
