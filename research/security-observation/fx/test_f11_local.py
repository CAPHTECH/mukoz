#!/usr/bin/env python3
"""Negative controls using actual F11 local records, no rerunning the subject or ptrace."""
import json
import re
import unittest
from pathlib import Path

import f11_local as F

ROOT = Path(__file__).resolve().parent
RUN = ROOT / 'runs/f11-local-2'
CONTRACT = json.loads((ROOT / 'contract/F11.json').read_text())
CASE = next(c for c in CONTRACT['cases'] if c['id'] == 'F11-p05')


def verdict(text, mode='c'):
    _, out = F.claims(CONTRACT, CASE, text, mode)
    return next(c['eval'] for c in out['claims'] if c['property'] == F.PROP)


class RecordedF11(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.xml = (RUN / 'r1/c/V/F11-p05/vg.xml').read_text()
        cls.raw = (RUN / 'r1/native/V/F11-p05/stdout').read_bytes()

    def test_read_free_and_allocation_are_distinct(self):
        ev, rc = F.C.M.parse(self.xml)
        self.assertEqual(len(ev), 1)
        self.assertEqual(ev[0]['detail']['kind'], 'InvalidRead')
        self.assertEqual(ev[0]['detail']['free_frame']['fn'], 'free')
        self.assertEqual(ev[0]['detail']['alloc_frame']['fn'], 'calloc')
        self.assertEqual(rc['gaps'], [])
        self.assertEqual(verdict(self.xml), F.C.A.VIO)

    def test_error_removed_disagrees_with_violation_label(self):
        stripped = re.sub(r'<error>.*?</error>', '', self.xml, flags=re.S)
        self.assertEqual(verdict(stripped), F.C.A.SAT)
        self.assertNotEqual(verdict(stripped), verdict(self.xml))

    def test_incomplete_error_is_not_success(self):
        cut = self.xml[:self.xml.index('<error>') + 8]
        self.assertEqual(verdict(cut), F.C.A.INC)

    def test_complete_error_survives_truncation(self):
        cut = self.xml[:self.xml.index('</error>') + len('</error>')]
        self.assertEqual(verdict(cut), F.C.A.VIO)

    def test_clean_truncated_xml_is_not_success(self):
        xml = (RUN / 'r1/c/R/F11-p05/vg.xml').read_text()
        self.assertEqual(verdict(xml.replace('</valgrindoutput>', '')), F.C.A.INC)

    def test_no_memory_capability_stays_unevaluated(self):
        self.assertEqual(verdict('', 'native'), F.C.A.NOT)
        self.assertEqual(verdict('', 'c0'), F.C.A.NOT)

    def test_response_and_memory_verdict_are_separate(self):
        self.assertTrue(F.response_ok(CONTRACT, CASE, self.raw))
        self.assertEqual(verdict(self.xml), F.C.A.VIO)
        self.assertFalse(F.response_ok(CONTRACT, CASE, self.raw.replace(b'alpha', b'xxxxx')))

    def test_response_length_mismatch_is_detected(self):
        self.assertFalse(F.response_ok(CONTRACT, CASE, self.raw + b'PASS'))

    def test_attribution_is_process_group_not_namespace(self):
        obs, out = F.claims(CONTRACT, CASE, self.xml, 'c')
        self.assertEqual(obs['cases'][0]['events'][0]['attribution']['case'], 'exclusive_process_group')
        self.assertEqual(out['schema'], 'mukoz.external_claims/0')


if __name__ == '__main__':
    unittest.main()
