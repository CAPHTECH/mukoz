#!/usr/bin/env python3
"""Run security examples through ordinary Mukoz checks and replay each announced violation.
No native execution or VM. Fresh output directory required. The JSON matrix is the test oracle.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def verify(case, result):
    """Check observation completeness, the announced property and caller/callee blame."""
    errors = []
    if result.get('ok') is not True or not isinstance(result.get('data'), dict):
        return ['check failed to produce observations']
    data = result['data']
    assessment = data.get('assessment', {})
    if assessment.get('admission') != case['admission']:
        errors.append(f"admission {assessment.get('admission')} != {case['admission']}")
    scope = assessment.get('scope', {})
    count = scope.get('cases_completed', 0)
    if count < 1 or scope.get('cases_skipped', 0) or count != scope.get('cases_planned'):
        errors.append('empty or incomplete execution')
    if not scope.get('engine_qualification', {}).get('qualified'):
        errors.append('engine not qualified')
    claims = {c['property']: c for c in data.get('claims', [])}
    if case['admission'] == 'ACCEPT_WITHIN_SCOPE' and (not claims or any(c.get('evaluation') != 'SATISFIED_IN_SCOPE' for c in claims.values())):
        errors.append('accepted control has missing or unsatisfied claims')
    prop = case.get('property')
    if prop and (claims.get(prop, {}).get('evaluation') != 'VIOLATED' or claims.get(prop, {}).get('cases_violated', 0) < 1):
        errors.append(f'missing violated claim: {prop}')
    if prop:
        findings = [f for f in data.get('findings', []) if f.get('property') == prop]
        if not findings or any(not f.get('counterexample_id') for f in findings):
            errors.append('missing replayable counterexample')
        if case.get('blame') and (not findings or any((f.get('detail') or {}).get('blame') != case['blame'] for f in findings)):
            errors.append('incorrect caller/callee blame')
    for p in case.get('satisfied', []):
        c = claims.get(p, {})
        if c.get('evaluation') != 'SATISFIED_IN_SCOPE' or c.get('cases_satisfied') != count:
            errors.append(f'functional control not satisfied: {p}')
    for p in case.get('forbid_violations', []):
        if claims.get(p, {}).get('evaluation') == 'VIOLATED':
            errors.append(f'wrongly blamed: {p}')
    if case.get('reason') and not any(case['reason'] in r for r in assessment.get('reasons', [])):
        errors.append('missing HOLD reason')
    return errors


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('out', type=Path)
    p.add_argument('--mukoz', type=Path, default=ROOT / 'target/release/mukoz')
    p.add_argument('--engine', type=Path)
    a = p.parse_args()
    matrix = json.loads((HERE / 'cases.json').read_text())
    cases = matrix['cases']
    by_id = {c['id']: c for c in cases}
    if len(by_id) != len(cases):
        raise ValueError('duplicate case IDs')
    for c in cases:
        if c.get('control') and (by_id[c['control']]['admission'] != 'ACCEPT_WITHIN_SCOPE' or by_id[c['control']]['isa'] != c['isa']):
            raise ValueError('invalid positive control')
    out = a.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    if a.engine:
        env['MUKOZ_EMU'] = str(a.engine.resolve())
    rows, problems = [], []
    for case in cases:
        d = out / case['id']
        d.mkdir()
        cmd = [str(a.mukoz.resolve()), 'check', str(ROOT / case['suite']), '--store', str(d / 'store')]
        if case.get('artifact'):
            cmd += ['--artifact', str(ROOT / case['artifact'])]
        for name, path in case.get('module', {}).items():
            cmd += ['--module', f'{name}={ROOT / path}']
        (d / 'command.json').write_text(json.dumps(cmd) + '\n')
        proc = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True, timeout=90)
        (d / 'check.json').write_text(proc.stdout)
        (d / 'stderr').write_text(proc.stderr)
        result = json.loads(proc.stdout)
        issues = verify(case, result)
        replay_ok = None
        if case.get('property') and not issues:
            finding = next(f for f in result['data']['findings'] if f['property'] == case['property'])
            cmd = [str(a.mukoz.resolve()), 'replay', finding['counterexample_id'], '--store', str(d / 'store')]
            replay = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True, timeout=90)
            (d / 'replay.json').write_text(replay.stdout)
            r = json.loads(replay.stdout)
            claim = next((x for x in (r.get('data') or {}).get('claims', []) if x['property'] == case['property']), {})
            replay_ok = r.get('ok') is True and claim.get('evaluation') == 'VIOLATED'
            if case.get('blame'):
                replay_ok = replay_ok and (claim.get('detail') or {}).get('blame') == case['blame']
            if not replay_ok:
                issues.append('announced property/blame did not replay')
        data = result.get('data') or {}
        row = {'id': case['id'], 'category': case['category'], 'isa': case['isa'],
               'admission': (data.get('assessment') or {}).get('admission'), 'property': case.get('property'),
               'blame': case.get('blame'), 'replay_verified': replay_ok, 'problems': issues,
               'subject_context': (data.get('assessment') or {}).get('subject_context'),
               'scope': (data.get('assessment') or {}).get('scope')}
        rows.append(row)
        problems += [case['id'] + ': ' + e for e in issues]
        print(f'{case["id"]}: {row["admission"]} ' + ('OK' if not issues else str(issues)), flush=True)
    report = {'schema': 'mukoz.security_examples.report/0', 'checks': len(rows),
              'replays': sum(r['replay_verified'] is True for r in rows), 'rows': rows, 'problems': problems,
              'matrix_sha256': hashlib.sha256((HERE / 'cases.json').read_bytes()).hexdigest(),
              'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'mukoz_sha256': hashlib.sha256(a.mukoz.read_bytes()).hexdigest()}
    (out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'{len(rows)} checks, {report["replays"]} replays, {len(problems)} problems')
    return bool(problems)


if __name__ == '__main__':
    sys.exit(main())
