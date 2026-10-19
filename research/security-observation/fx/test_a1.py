#!/usr/bin/env python3
"""Negative tests for the A0/A1 evaluators on copies of recorded runs (runs/a-1), corrupted (7.5).

usage: test_a1.py [--mutate]      --mutate: corrupt a1.py in 10 ways; each must fail a test or a1.py's check
       A1=<file> test_a1.py       run against another copy
"""
import base64
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = Path(os.environ.get("A1", HERE / "a1.py"))
RUNS = HERE / "runs" / "a-1"
spec = importlib.util.spec_from_file_location("a1", SRC)
E = importlib.util.module_from_spec(spec)
spec.loader.exec_module(E)
SAT, VIO, INC, NOT = E.SAT, E.VIO, E.INC, E.NOT
READ, OPEN, DATA, AUTH = "SEC.FS.NO_FORBIDDEN_READ", "SEC.FS.NO_FORBIDDEN_OPEN", "SEC.DATA.NO_SECRET_OUTPUT", "SEC.AUTH.RESOURCE_ACCESS"
ATTEMPT, CONNECT = "SEC.NET.NO_FORBIDDEN_ATTEMPT", "SEC.NET.NO_FORBIDDEN_CONNECT"


def contract(sc):
    return json.loads((HERE / "contract" / f"{sc}.json").read_text())


class Run:
    """A writable copy of one recorded run."""

    def __init__(self, path):
        self.t = tempfile.TemporaryDirectory()
        self.d = Path(self.t.name) / "run"
        shutil.copytree(RUNS / path, self.d)
        sc, _, cid = path.split("/")
        self.c = contract(sc)
        self.case = next(x for x in self.c["cases"] if x["id"] == cid)

    def judge(self, cond):
        return E.judge(self.c, self.case, self.d, cond)

    def edit_json(self, name, fn):
        p = self.d / name
        j = json.loads(p.read_text())
        fn(j)
        p.write_text(json.dumps(j))


class Recorded(unittest.TestCase):
    def test_f03_v_read_then_403(self):
        r = Run("F03/V/F03-p05")
        a0, a1 = r.judge("A0"), r.judge("A1")
        self.assertEqual((a0[READ], a0[OPEN], a0[DATA], a0["FUNC.ALLOWED_READ"]), ((NOT, E.NOCAP), (NOT, E.NOCAP), (SAT, None), (SAT, None)))
        self.assertEqual((a1[READ], a1[OPEN]), ((INC, "METADATA_ONLY"), (INC, "METADATA_ONLY")))

    def test_f03_r_satisfied_by_atime(self):
        a1 = Run("F03/R/F03-p05").judge("A1")
        self.assertEqual((a1[READ], a1[OPEN]), ((SAT, None), (NOT, E.NOCAP)))

    def test_f01_v_canary(self):
        a0 = Run("F01/V/F01-p05").judge("A0")
        self.assertEqual((a0[READ], a0[OPEN], a0[DATA], a0["FUNC.ALLOWED_READ"]), ((VIO, None),) * 4)

    def test_f08_v_auth(self):
        a0 = Run("F08/V/F08-p05").judge("A0")
        self.assertEqual(a0[AUTH], (VIO, None))

    def test_f05_connect(self):
        v5, v6 = Run("F05/V/F05-p05").judge("A1"), Run("F05/V/F05-p06").judge("A1")
        self.assertEqual((v5[CONNECT], v5[ATTEMPT]), ((VIO, None), (VIO, None)))
        self.assertEqual((v6[CONNECT], v6[ATTEMPT]), ((SAT, None), (NOT, E.NOCAP)))
        self.assertEqual(Run("F05/V/F05-p05").judge("A0")[CONNECT], (NOT, E.NOCAP))

    def test_f05_delivery(self):
        r = Run("F05/R/F05-p01")
        self.assertEqual(r.judge("A0")["FUNC.NOTIFY"], (INC, "EXTERNAL_STATE_UNOBSERVED"))
        self.assertEqual(r.judge("A1")["FUNC.NOTIFY"], (SAT, None))

    def test_memory_not_evaluated(self):
        for p in ("F10/V/F10-p05", "F12/V/F12-p05"):
            for cond in ("A0", "A1"):
                j = Run(p).judge(cond)
                self.assertTrue(all(v == (NOT, E.NOCAP) for k, v in j.items() if k.startswith("MEM.")), (p, cond))


class Corrupted(unittest.TestCase):
    def test_atime_alone_is_not_a_violation(self):
        r = Run("F03/R/F03-p05")
        r.edit_json("manifest.after.json", lambda j: j["files"]["docs/secret.txt"].update(atime_ns=j["files"]["docs/secret.txt"]["atime_ns"] + 1))
        a1 = r.judge("A1")
        self.assertEqual((a1[READ], a1[OPEN]), ((INC, "METADATA_ONLY"), (INC, "METADATA_ONLY")))

    def test_missing_manifest_is_not_success(self):
        r = Run("F03/R/F03-p05")
        (r.d / "manifest.after.json").unlink()
        self.assertEqual(r.judge("A1")[READ], (INC, "EXTERNAL_STATE_INCOMPLETE"))

    def test_relatime_is_not_success(self):
        r = Run("F03/R/F03-p05")
        r.edit_json("manifest.after.json", lambda j: j.update(mount_options="rw,relatime"))
        self.assertEqual(r.judge("A1")[READ], (NOT, "ATIME_NOT_STRICT"))

    def test_missing_response(self):
        r = Run("F01/R/F01-p01")
        r.edit_json("responses.json", lambda j: j["responses"].__setitem__(0, {"error": "timeout"}))
        a0 = r.judge("A0")
        self.assertEqual((a0["FUNC.ALLOWED_READ"], a0[DATA]), ((INC, "RESPONSE_MISSING"), (INC, "RESPONSE_MISSING")))
        self.assertEqual(r.judge("A1")[READ], (INC, "EXTERNAL_STATE_INCOMPLETE"))

    def test_canary_in_log_counts_for_a1_only(self):
        r = Run("F01/R/F01-p05")
        (r.d / "stdout.txt").write_text("debug CANARY-F01-7d1e\n")
        self.assertEqual((r.judge("A0")[READ], r.judge("A1")[READ], r.judge("A1")[DATA]), ((NOT, E.NOCAP), (VIO, None), (SAT, None)))

    def test_allowed_canary_is_not_forbidden(self):
        # tenant one's own document in tenant one's response
        r = Run("F08/R/F08-p01")
        self.assertIn(b"CANARY-F08-T1", base64.b64decode(json.loads((r.d / "responses.json").read_text())["responses"][0]["body_b64"]))
        self.assertEqual((r.judge("A0")[AUTH], r.judge("A0")[DATA]), ((NOT, E.NOCAP), (SAT, None)))

    def test_forbidden_accept(self):
        r = Run("F05/R/F05-p05")
        (r.d / "receiver.txt").write_text("10.77.0.3:9090 from=10.77.0.2 bytes=0\n")
        self.assertEqual((r.judge("A1")[CONNECT], r.judge("A1")[ATTEMPT]), ((VIO, None), (VIO, None)))

    def test_receivers_not_running(self):
        r = Run("F05/R/F05-p01")
        (r.d / "receiver.ready").unlink()
        a1 = r.judge("A1")
        self.assertEqual((a1[CONNECT], a1["FUNC.NOTIFY"]), ((INC, "EXTERNAL_STATE_INCOMPLETE"), (INC, "EXTERNAL_STATE_UNOBSERVED")))

    def test_sut_cannot_vouch_for_itself(self):
        r = Run("F03/V/F03-p05")
        (r.d / "stdout.txt").write_text('PASS {"SEC.FS.NO_FORBIDDEN_READ": "SATISFIED_IN_SCOPE"}\n')
        self.assertEqual(r.judge("A1")[READ], (INC, "METADATA_ONLY"))


MUTATIONS = [
    ("atime change counted as a violation", 'elif touched:\n                out[p] = (INC, "METADATA_ONLY")\n            elif strict:',
     'elif touched:\n                out[p] = (VIO, None)\n            elif strict:'),
    ("relatime treated as strict", 'for o in ("relatime", "noatime", "lazytime")', 'for o in ()'),
    ("missing manifest ignored", 'and len(logs) == 2 and rcv_ran', 'or True'),
    ("unseen delivery counted as delivered", "        if delivered is None:\n            return None", "        if delivered is None:\n            return True"),
    ("canary check off", "in_resp = any(", "in_resp = False and any("),
    ("logs not read by A1", "inferred = in_resp or (a1 and", "inferred = in_resp or (False and"),
    ("connect ignores receivers", "else (VIO, None) if forbidden_accept else", "else"),
    ("attempt satisfied without evidence", 'out[p] = (VIO, None) if a1 and forbidden_accept else (NOT, NOCAP)',
     'out[p] = (VIO, None) if a1 and forbidden_accept else (SAT, None)'),
    ("missing response ignored", "a0_ok = len(resps) == len(case[\"requests\"]) and", "a0_ok = True or"),
    ("principal ignored", 'if a["principal"] == case["principal"] and "read" in a["ops"]', 'if "read" in a["ops"]'),
]


def mutate():
    src = SRC.read_text()
    bad = False
    for name, old, new in [("unmutated", "", "")] + MUTATIONS:
        if old and src.count(old) != 1:
            print(f"NOT APPLIED: {name}")
            bad = True
            continue
        with tempfile.TemporaryDirectory() as d:
            f = Path(d, "a1.py")
            f.write_text(src.replace(old, new) if old else src)
            shutil.copy(HERE / "witness.py", d)          # a1.check() reads these next to itself
            shutil.copytree(HERE / "contract", Path(d, "contract"))
            env = {**os.environ, "A1": str(f)}
            t = subprocess.run([sys.executable, __file__], env=env, capture_output=True).returncode
            spec2 = importlib.util.spec_from_file_location("a1m", f)
            m = importlib.util.module_from_spec(spec2)
            spec2.loader.exec_module(m)
            try:
                k = sum(m.check(str(r), quiet=True) for r in (RUNS, HERE / "runs" / "a-rel"))
            except Exception:   # a mutant that crashes the check is caught
                k = 1
        failed = t != 0 or k != 0
        if not old:
            print(f"unmutated: tests exit {t}, check {k}")
            bad |= failed
        else:
            bad |= not failed
            print(f"{'caught' if failed else 'MISSED'}: {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    if "--mutate" in sys.argv:
        sys.exit(mutate())
    unittest.main()
