#!/usr/bin/env python3
"""Tests for the Memcheck normalizer on a recorded XML (WP0.7 sbx/runs/s6) and corrupted copies of it.

usage: test_memcheck.py              run against ./memcheck.py
       test_memcheck.py --mutate     corrupt memcheck.py in 9 ways; exit 0 iff every mutation fails a test
       MEMCHECK=<file> test_memcheck.py   run against another copy
"""
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = Path(os.environ.get("MEMCHECK", HERE / "memcheck.py"))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


M = load("memcheck_norm", SRC)
A = load("adapter", HERE.parent / "adapter" / "adapter.py")
RECORDED = (HERE.parent / "sbx" / "runs" / "s6" / "sbx-vg-heapover" / "vg.xml").read_text()
# The recorded run predates --error-limit=no; the baseline adds it so that the receipt has no gap.
BASE = RECORDED.replace("<arg>--xml-fd=4</arg>", "<arg>--xml-fd=4</arg>\n    <arg>--error-limit=no</arg>")
HEAP = "external.mem.heap.addressable"
BINDING = {"schema": "secobs.binding/0", "resources": [], "allow": [], "net_allowed": [], "exec_allowed": [],
           "properties": {HEAP: "mem.heap.addressable"}}
BRAW = json.dumps(BINDING).encode()
STRACE_OK = {"start": "before_sut_exec", "end": "drained", "gaps": [], "lost_events": 0, "capabilities": ["fs", "net", "proc"]}


def verdict(text):
    ev, rc = M.parse(text)
    o = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(BRAW).hexdigest(),
         "cases": [{"case_id": "c", "principal": "p", "requests_sent": 1, "receipt": rc, "events": ev}]}
    out, _ = A.convert(BRAW, BINDING, json.dumps(o).encode(), o, "0" * 64, "independent")
    c = next(x for x in out["claims"] if x["property"] == HEAP)
    return c["eval"], c["reason"]


def no_errors(t):
    return re.sub(r"<error>.*?</error>", "", t, flags=re.S)


class Recorded(unittest.TestCase):
    def test_recorded_error(self):
        ev, rc = M.parse(RECORDED)
        self.assertEqual(len(ev), 1)
        d = ev[0]["detail"]
        self.assertEqual((d["kind"], d["frame"]["fn"], d["frame"]["line"], d["alloc_frame"]["fn"]), ("InvalidWrite", "c_heapover", "150", "malloc"))
        self.assertIn("0 bytes after a block of size 16", d["auxwhat"])
        self.assertEqual((ev[0]["evidence_origin"], ev[0]["subject"]["pid"]), ("memcheck", 32))
        self.assertEqual((rc["start"], rc["end"], rc["capabilities"]), ("before_sut_exec", "drained", ["memcheck"]))
        self.assertEqual([g["reason"] for g in rc["gaps"]], ["error_limit_on"])

    def test_baseline_violated_and_clean_satisfied(self):
        self.assertEqual(M.parse(BASE)[1]["gaps"], [])
        self.assertEqual(verdict(BASE), (A.VIO, None))
        self.assertEqual(verdict(no_errors(BASE)), (A.SAT, None))

    def test_error_limit_holds_a_clean_run(self):
        self.assertEqual(verdict(no_errors(RECORDED)), (A.INC, "MONITORING_INCOMPLETE"))

    def test_count_from_errorcounts(self):
        t = BASE.replace("<count>1</count>\n    <unique>0x0</unique>", "<count>7</count>\n    <unique>0x0</unique>")
        self.assertEqual(M.parse(t)[0][0]["detail"]["count"], 7)

    def test_leak_kind_is_not_a_violation(self):
        self.assertEqual(verdict(BASE.replace("<kind>InvalidWrite</kind>", "<kind>Leak_DefinitelyLost</kind>")), (A.SAT, None))


class Truncated(unittest.TestCase):
    def test_cut_after_error_keeps_counterexample(self):
        t = BASE[: BASE.index("</error>") + len("</error>")]
        ev, rc = M.parse(t)
        self.assertEqual((len(ev), rc["end"]), (1, "process_exit_unobserved"))
        self.assertIn("xml_truncated", [g["reason"] for g in rc["gaps"]])
        self.assertEqual(verdict(t), (A.VIO, None))

    def test_cut_inside_error_is_not_success(self):
        t = BASE[: BASE.index("<auxwhat>")]
        self.assertEqual(M.parse(t)[0], [])
        self.assertEqual(verdict(t), (A.INC, "MONITORING_END_MISSING"))

    def test_running_state_only_is_not_drained(self):
        t = no_errors(BASE).replace("<state>FINISHED</state>", "<state>RUNNING</state>")
        self.assertEqual(verdict(t), (A.INC, "MONITORING_END_MISSING"))

    def test_missing_preamble_is_no_start(self):
        t = re.sub(r"<preamble>.*?</preamble>", "", no_errors(BASE), flags=re.S)
        self.assertEqual(verdict(t), (A.INC, "MONITORING_START_MISSING"))


class Gaps(unittest.TestCase):
    def test_suppressed_errors(self):
        t = no_errors(BASE).replace("<suppcounts>\n</suppcounts>",
                                    "<suppcounts>\n  <pair>\n    <count>2</count>\n    <name>x</name>\n  </pair>\n</suppcounts>")
        self.assertEqual([g["reason"] for g in M.parse(t)[1]["gaps"]], ["errors_suppressed"])
        self.assertEqual(verdict(t), (A.INC, "MONITORING_INCOMPLETE"))

    def test_sigill(self):
        t = no_errors(BASE).replace("<status>\n  <state>FINISHED",
                                    "<fatal_signal>\n  <tid>1</tid>\n  <signo>4</signo>\n  <signame>SIGILL</signame>\n</fatal_signal>\n<status>\n  <state>FINISHED")
        self.assertEqual(verdict(t), (A.INC, "MONITORING_INCOMPLETE"))

    def test_sigterm_stop_is_not_a_gap(self):
        t = no_errors(BASE).replace("<status>\n  <state>FINISHED",
                                    "<fatal_signal>\n  <tid>1</tid>\n  <signo>15</signo>\n  <signame>SIGTERM</signame>\n</fatal_signal>\n<status>\n  <state>FINISHED")
        self.assertEqual(verdict(t), (A.SAT, None))

    def test_other_tool_has_no_capability(self):
        t = no_errors(BASE).replace("<protocoltool>memcheck</protocoltool>", "<protocoltool>none</protocoltool>")
        self.assertEqual(verdict(t), (A.NOT, "REQUIRED_CAPABILITY_UNAVAILABLE"))


class Merge(unittest.TestCase):
    def test_merge(self):
        _, mc = M.parse(BASE)
        m = M.merge(STRACE_OK, mc)
        self.assertEqual((m["start"], m["end"], m["capabilities"]), ("before_sut_exec", "drained", ["fs", "memcheck", "net", "proc"]))
        _, cut = M.parse(BASE[: BASE.index("</error>") + 8])
        m = M.merge(STRACE_OK, cut)
        self.assertEqual(m["end"], "process_exit_unobserved")
        self.assertEqual([g["reason"] for g in m["gaps"]], ["xml_truncated"])
        m = M.merge(dict(STRACE_OK, end="forced_stop"), mc)
        self.assertEqual(m["end"], "forced_stop")
        m = M.merge(dict(STRACE_OK, start=None), mc)
        self.assertIsNone(m["start"])


MUTATIONS = [
    ("truncation ignored", 'complete = text.rstrip().endswith("</valgrindoutput>")', "complete = True"),
    ("error limit ignored", 'if not vargs or "<arg>--error-limit=no</arg>" not in vargs.group(1):', "if False:"),
    ("suppressions ignored", 'if supp and re.search(r"<count>[1-9]", supp.group(1)):', "if False:"),
    ("any fatal signal is a gap", 'if sig and sig.group(1) == "SIGILL":', "if sig:"),
    ("SIGILL ignored", 'if sig and sig.group(1) == "SIGILL":', "if False:"),
    ("capability whatever the tool", '"capabilities": ["memcheck"] if tool == "memcheck" else []', '"capabilities": ["memcheck"]'),
    ("RUNNING counted as finished", 'states[-1] == "FINISHED"', 'states[-1] in ("FINISHED", "RUNNING")'),
    ("merge takes the first end", 'end = a["end"] if a["end"] == b["end"] == "drained" else (a["end"] if a["end"] != "drained" else b["end"])',
     'end = a["end"]'),
    ("errors not reported", "        events.append(", "        0 and events.append("),
]


def mutate():
    src = SRC.read_text()
    r = subprocess.run([sys.executable, __file__], capture_output=True)
    bad = r.returncode != 0
    print(f"unmutated: exit {r.returncode}")
    for name, old, new in MUTATIONS:
        if src.count(old) != 1:
            print(f"NOT APPLIED: {name}")
            bad = True
            continue
        with tempfile.TemporaryDirectory() as d:
            f = Path(d, "memcheck.py")
            f.write_text(src.replace(old, new))
            r = subprocess.run([sys.executable, __file__], env={**os.environ, "MEMCHECK": str(f)}, capture_output=True, text=True)
        caught = r.returncode != 0
        bad |= not caught
        print(f"{'caught' if caught else 'MISSED'}: {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    if "--mutate" in sys.argv:
        sys.exit(mutate())
    unittest.main()
