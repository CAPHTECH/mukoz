#!/usr/bin/env python3
"""Negative tests for the E1 aggregation on copies of the smoke runs (runs/e1-smoke: V, case p05, seed 1,
both implementations, all five conditions), corrupted.

usage: test_e1.py [--mutate]      --mutate: corrupt e1.py in 5 ways; each must fail a test
"""
import contextlib
import importlib.util
import io
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = Path(os.environ.get("E1", HERE / "e1.py"))
RUNS, CDIR = HERE / "runs" / "e1-smoke", HERE / "contract-e1"
spec = importlib.util.spec_from_file_location("e1", SRC)
E = importlib.util.module_from_spec(spec)
spec.loader.exec_module(E)


def agg(root):
    with contextlib.redirect_stdout(io.StringIO()):
        E.main(str(root), str(CDIR))
    return E.LAST


class Copy:
    def __enter__(self):
        self.t = tempfile.TemporaryDirectory()
        self.d = Path(self.t.name) / "runs"
        shutil.copytree(RUNS, self.d)
        return self.d

    def __exit__(self, *a):
        self.t.cleanup()


class Smoke(unittest.TestCase):
    def test_recorded(self):
        r = agg(RUNS)
        names = lambda k: sorted(f"{sc}/{i}" for sc, i in r["detected"][k])
        self.assertEqual(names("a0"), ["F01/1", "F01/2", "F08/1", "F08/2"])
        self.assertEqual(names("a1"), ["F01/1", "F01/2", "F05/1", "F05/2", "F08/1", "F08/2"])
        self.assertEqual(names("b"), names("a1")[:2] + ["F03/1", "F03/2"] + names("a1")[2:])
        self.assertEqual(names("c"), names("b") + ["F10/1", "F10/2"])
        self.assertEqual(dict(r["issues"]), {})

    def test_memory_truth_from_gdb(self):
        # Without the gdb hit, C's heap VIOLATED on F10 impl 2 must show up as a false VIOLATED.
        with Copy() as d:
            for k in ("a0", "a1"):
                g = d / f"i2-s1-{k}" / "F10" / "V" / "F10-p05" / "gdb.txt"
                g.write_text(g.read_text().replace("WITNESS_HIT", "-"))
            r = agg(d)
            self.assertEqual(r["issues"]["false VIOLATED c"], 1)
            self.assertNotIn(("F10", 2), r["detected"]["c"])

    def test_low_level_false_success_is_reported(self):
        # B's strace says nothing was opened; A1 has nothing either: the false SATISFIED must count,
        # and a remaining canary in the response gives a contradiction instead.
        with Copy() as d:
            (d / "i1-s1-b" / "F03" / "V" / "F03-p05" / "strace.txt").write_text(
                (d / "i1-s1-b" / "F03" / "V" / "F03-p05" / "strace.txt").read_text().replace("docs/secret.txt", "docs/pub1.txt"))
            r = agg(d)
            self.assertNotIn(("F03", 1), r["detected"]["b"])
            self.assertGreaterEqual(sum(v for k, v in r["issues"].items() if k.startswith("false SATISFIED b")), 1)

    def test_missing_condition(self):
        with Copy() as d:
            shutil.rmtree(d / "i1-s1-c" / "F10")
            r = agg(d)
            self.assertEqual(r["issues"]["block missing conditions"], 1)
            self.assertNotIn(("F10", 1), r["detected"]["c"])

    def test_behavior_divergence(self):
        with Copy() as d:
            (d / "i1-s1-c0" / "F05" / "V" / "F05-p05" / "receiver.txt").write_text("")
            r = agg(d)
            self.assertGreaterEqual(r["issues"]["behavior differs across conditions (11.4)"], 1)


MUTATIONS = [
    ("low-level verdicts ignored", "out[p], k = combine(ext.get(p), low.get(p))", "out[p], k = combine(ext.get(p))"),
    ("memory truth ignored", "truth = seen | memtruth", "truth = seen"),
    ("false VIOLATED not counted", 'if r["eval"] == VIO and not r["violated"]:', "if False:"),
    ("missing conditions not counted", "        if missing:\n", "        if False:\n"),
    ("divergence not counted", "if len(set(truths.values())) > 1:", "if False:"),
]


def mutate():
    src = SRC.read_text()
    bad = subprocess.run([sys.executable, __file__], capture_output=True).returncode != 0
    print(f"unmutated: {'FAIL' if bad else 'ok'}")
    for name, old, new in MUTATIONS:
        if src.count(old) != 1:
            print(f"NOT APPLIED: {name}")
            bad = True
            continue
        f = HERE / "_e1_mutant.py"   # next to e1.py: it loads witness.py, a1.py, c_check.py beside itself
        try:
            f.write_text(src.replace(old, new))
            r = subprocess.run([sys.executable, __file__], env={**os.environ, "E1": str(f)}, capture_output=True)
        finally:
            f.unlink()
        caught = r.returncode != 0
        bad |= not caught
        print(f"{'caught' if caught else 'MISSED'}: {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    if "--mutate" in sys.argv:
        sys.exit(mutate())
    unittest.main()
