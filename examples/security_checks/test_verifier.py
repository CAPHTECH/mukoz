#!/usr/bin/env python3
"""Corrupt recorded CLI results to check that the example verifier cannot report false success.
Restore archived records with research/security-observation/restore_evidence.py first.
"""
import json
import unittest
from pathlib import Path
import run as verifier

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
RECORDS = ROOT / 'research/security-observation/security-checks/runs/matrix-2'
CASES = {c['id']: c for c in json.loads((HERE / 'cases.json').read_text())['cases']}


def sample(case_id):
    return CASES[case_id], json.loads((RECORDS / case_id / 'check.json').read_text())


class RecordedVerifier(unittest.TestCase):
    def test_all_recorded_results_match_the_prior_matrix(self):
        for cid in CASES:
            with self.subTest(case=cid):
                case, result = sample(cid)
                self.assertEqual(verifier.verify(case, result), [])

    def test_missing_announced_claim_is_detected(self):
        c, r = sample('abi.clobber.x86_64')
        r['data']['claims'] = [x for x in r['data']['claims'] if x['property'] != c['property']]
        self.assertTrue(verifier.verify(c, r))

    def test_wrong_caller_callee_blame_is_detected(self):
        c, r = sample('module.caller_requires.x86_64')
        for f in r['data']['findings']:
            if f['property'] == c['property']:
                f['detail']['blame'] = 'callee'
        self.assertTrue(verifier.verify(c, r))

    def test_an_abi_violation_must_preserve_the_functional_control(self):
        c, r = sample('abi.clobber.x86_64')
        for claim in r['data']['claims']:
            if claim['property'] == 'arith.add64/sum':
                claim['evaluation'], claim['cases_satisfied'] = 'VIOLATED', 0
        self.assertTrue(verifier.verify(c, r))

    def test_empty_execution_is_not_success(self):
        c, r = sample('abi.correct.x86_64')
        r['data']['assessment']['scope'].update(cases_completed=0, cases_planned=0)
        self.assertTrue(verifier.verify(c, r))

    def test_skipped_cases_are_detected(self):
        c, r = sample('abi.correct.x86_64')
        r['data']['assessment']['scope']['cases_skipped'] = 1
        self.assertTrue(verifier.verify(c, r))

    def test_unqualified_engine_is_not_success(self):
        c, r = sample('abi.correct.x86_64')
        r['data']['assessment']['scope']['engine_qualification']['qualified'] = False
        self.assertTrue(verifier.verify(c, r))

    def test_unsupported_instruction_is_not_a_forbidden_effect(self):
        c, r = sample('guard.unsupported.x86_64')
        r['data']['assessment']['admission'] = 'REJECT'
        for claim in r['data']['claims']:
            if claim['property'] == 'effects.no_forbidden':
                claim['evaluation'] = 'VIOLATED'
        self.assertTrue(verifier.verify(c, r))

    def test_missing_claims_in_an_accepted_result_are_detected(self):
        c, r = sample('abi.correct.x86_64')
        r['data']['claims'] = []
        self.assertTrue(verifier.verify(c, r))

    def test_inconclusive_claim_in_an_accepted_result_is_detected(self):
        c, r = sample('abi.correct.x86_64')
        r['data']['claims'][0]['evaluation'] = 'INCONCLUSIVE'
        self.assertTrue(verifier.verify(c, r))

    def test_missing_counterexample_is_detected(self):
        c, r = sample('abi.clobber.x86_64')
        for finding in r['data']['findings']:
            finding['counterexample_id'] = None
        self.assertTrue(verifier.verify(c, r))


if __name__ == '__main__':
    unittest.main()
