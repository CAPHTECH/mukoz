#!/usr/bin/env python3
"""Negative tests for the adapter on artificial events (DESIGN.md 7.5).

usage: test_adapter.py            run against ./adapter.py
       ADAPTER=<file> test_adapter.py   run against another copy (mutate.py uses this)
"""
import copy
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = Path(os.environ.get("ADAPTER", HERE / "adapter.py"))
spec = importlib.util.spec_from_file_location("adapter", SRC)
A = importlib.util.module_from_spec(spec)
spec.loader.exec_module(A)

CTX = "ab" * 32
READ, OPEN, WRITE = "external.sec.fs.no_forbidden_read", "external.sec.fs.no_forbidden_open", "external.sec.fs.no_forbidden_write"
ATTEMPT, CONNECT = "external.sec.net.no_forbidden_attempt", "external.sec.net.no_forbidden_connect"
EXEC, HEAP = "external.sec.proc.allowed_exec_only", "external.mem.heap.addressable"
REGION = "external.mem.region.read_authorized"

BINDING = {
    "schema": "secobs.binding/0",
    "resources": ["doc-public", "doc-secret", "audit-log"],
    "allow": [
        {"principal": "alice", "resource": "doc-public", "ops": ["read"]},
        {"principal": "bob", "resource": "doc-public", "ops": ["read"]},
        {"principal": "bob", "resource": "doc-secret", "ops": ["read"]},
    ],
    "net_allowed": ["10.77.0.1:8080"],
    "exec_allowed": ["/sut/app"],
    "properties": {
        OPEN: "fs.no_forbidden_open", READ: "fs.no_forbidden_read", WRITE: "fs.no_forbidden_write",
        ATTEMPT: "net.no_forbidden_attempt", CONNECT: "net.no_forbidden_connect",
        EXEC: "proc.allowed_exec_only", HEAP: "mem.heap.addressable",
    },
    "fills": {"effects.no_forbidden": [OPEN, READ, WRITE, ATTEMPT, CONNECT, EXEC]},
}
BRAW = json.dumps(BINDING).encode()


def ev(kind, res, ok=True, n=None, **kw):
    e = {"subject": {"role": "sut", "pid": 2}, "event_kind": kind, "stage": "completed" if ok else "failed",
         "resource_id": res, "result": {"success": ok}, "attribution": {"case": "exclusive_process_group"},
         "evidence_origin": "os_observer"}
    if n is not None:
        e["result"]["bytes"] = n
    e.update(kw)
    return e


def clean_case(cid="rnd-00000", principal="alice"):
    """A case where the SUT only does what alice may do, and the observation is complete."""
    return {
        "case_id": cid, "principal": principal, "requests_sent": 1,
        "receipt": {"start": "before_sut_exec", "end": "drained", "gaps": [], "lost_events": 0,
                    "capabilities": ["fs", "net", "proc", "memcheck"]},
        "events": [
            ev("process_exec", "/sut/app"),
            ev("file_open", "/usr/lib/libc.so.6", access="read"),
            ev("file_read", "/usr/lib/libc.so.6", n=832),
            ev("file_open", "doc-public", access="read"),
            ev("file_read", "doc-public", n=100),
            ev("file_open", "doc-secret", ok=False, access="read", result={"success": False, "errno": "EACCES"}),
            ev("net_connect", "10.77.0.1:8080"),
        ],
    }


def obs(*cases, binding_raw=BRAW):
    return {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(binding_raw).hexdigest(),
            "collectors": [{"name": "artificial", "version": "0"}], "cases": list(cases)}


def run(o, binding=BINDING, independence="independent"):
    oraw = json.dumps(o).encode()
    out, _ = A.convert(json.dumps(binding).encode(), binding, oraw, o, CTX, independence)
    return out


def table(out):
    return {(c["case_id"], c["property"]): (c["eval"], c["reason"]) for c in out["claims"]}


def get(o, prop, case="rnd-00000", **kw):
    return table(run(o, **kw))[(case, prop)]


class Baseline(unittest.TestCase):
    def test_clean_case_is_satisfied_everywhere(self):
        t = table(run(obs(clean_case())))
        self.assertEqual(len(t), len(BINDING["properties"]) + 1)
        for k, v in t.items():
            self.assertEqual(v, (A.SAT, None), k)

    def test_output_shape(self):
        out = run(obs(clean_case()))
        self.assertEqual(out["schema"], "mukoz.external_claims/0")
        self.assertEqual(out["subject_context"], CTX)
        self.assertEqual(out["properties"], list(BINDING["properties"]))
        self.assertEqual(out["independence"], "independent")
        self.assertIsNone(run(obs(clean_case()), independence="undeclared")["independence"])


class ReadCount(unittest.TestCase):
    """7.5: read > 0 -> read = -1 removes the successful-read violation."""

    def forbidden_read(self, n, ok=True):
        c = clean_case()
        c["events"].append(ev("file_read", "doc-secret", ok=ok, n=n))
        return obs(c)

    def test_positive_read_of_forbidden_resource_is_violation(self):
        self.assertEqual(get(self.forbidden_read(32), READ), (A.VIO, None))
        self.assertEqual(get(self.forbidden_read(32), "effects.no_forbidden"), (A.VIO, None))

    def test_failed_read_is_not_violation(self):
        self.assertEqual(get(self.forbidden_read(-1, ok=False), READ), (A.SAT, None))

    def test_zero_byte_read_is_not_violation(self):
        self.assertEqual(get(self.forbidden_read(0), READ), (A.SAT, None))

    def test_open_without_read_is_open_violation_only(self):
        c = clean_case()
        c["events"].append(ev("file_open", "doc-secret", access="read"))
        self.assertEqual(get(obs(c), OPEN), (A.VIO, None))
        self.assertEqual(get(obs(c), READ), (A.SAT, None))


class Sendfile(unittest.TestCase):
    """7.5: sendfile > 0 -> 0 removes the successful-read violation."""

    def xfer(self, n, src="doc-secret", dst="sock:10.77.0.1:8080"):
        c = clean_case()
        c["events"].append(ev("file_transfer", src, n=n, dest_resource_id=dst, mechanism="sendfile"))
        return obs(c)

    def test_positive_sendfile_from_forbidden_is_read_violation(self):
        self.assertEqual(get(self.xfer(4096), READ), (A.VIO, None))

    def test_zero_sendfile_is_not_violation(self):
        self.assertEqual(get(self.xfer(0), READ), (A.SAT, None))

    def test_transfer_into_protected_is_write_violation(self):
        self.assertEqual(get(self.xfer(4096, src="doc-public", dst="audit-log"), WRITE), (A.VIO, None))
        self.assertEqual(get(self.xfer(4096, src="doc-public", dst="audit-log"), READ), (A.SAT, None))


class Principal(unittest.TestCase):
    """7.5: changing an allowed principal to a forbidden one changes the authorization verdict."""

    def secret_read(self, principal):
        c = clean_case(principal=principal)
        c["events"].append(ev("file_read", "doc-secret", n=32))
        return obs(c)

    def test_allowed_principal_is_satisfied(self):
        self.assertEqual(get(self.secret_read("bob"), READ), (A.SAT, None))

    def test_forbidden_principal_is_violation(self):
        self.assertEqual(get(self.secret_read("alice"), READ), (A.VIO, None))

    def test_unknown_principal_is_violation(self):
        self.assertEqual(get(self.secret_read("mallory"), READ), (A.VIO, None))


class MissingObservation(unittest.TestCase):
    """7.5: missing start, missing middle, unresolved target, stale binding never yield success."""

    def assert_not_sat(self, o, reason, props=None):
        t = table(run(o))
        for p in props or list(BINDING["properties"]) + ["effects.no_forbidden"]:
            ev_, r = t[("rnd-00000", p)]
            self.assertNotEqual(ev_, A.SAT, p)
            self.assertEqual(r, reason, p)

    def test_start_missing(self):
        c = clean_case()
        c["receipt"]["start"] = None
        self.assert_not_sat(obs(c), "MONITORING_START_MISSING")

    def test_end_missing(self):
        c = clean_case()
        c["receipt"]["end"] = "timeout"
        self.assert_not_sat(obs(c), "MONITORING_END_MISSING")

    def test_gap_in_the_middle(self):
        c = clean_case()
        c["receipt"]["gaps"] = [{"from": 10, "to": 12}]
        self.assert_not_sat(obs(c), "MONITORING_INCOMPLETE")

    def test_lost_events(self):
        c = clean_case()
        c["receipt"]["lost_events"] = 3
        self.assert_not_sat(obs(c), "MONITORING_INCOMPLETE")

    def test_unresolved_resource(self):
        c = clean_case()
        c["events"].append(ev("file_read", None, n=32))
        self.assert_not_sat(obs(c), "ATTRIBUTION_UNRESOLVED", [READ])

    def test_uncertain_case_attribution(self):
        c = clean_case()
        e = ev("file_read", "doc-secret", n=32)
        e["attribution"]["case"] = "pid_guess"
        c["events"].append(e)
        self.assert_not_sat(obs(c), "ATTRIBUTION_UNRESOLVED", [READ])

    def test_stale_binding(self):
        self.assert_not_sat(obs(clean_case(), binding_raw=b"{}"), "BINDING_STALE")

    def test_stale_binding_does_not_produce_violation_either(self):
        c = clean_case()
        c["events"].append(ev("file_read", "doc-secret", n=32))
        self.assertEqual(get(obs(c, binding_raw=b"{}"), READ), (A.INC, "BINDING_STALE"))

    def test_unsupported_event_kind(self):
        c = clean_case()
        c["events"].append(ev("mmap_access", "doc-secret"))
        self.assert_not_sat(obs(c), "EVENT_UNSUPPORTED", [READ])

    def test_unsupported_mechanism_on_protected_resource(self):
        c = clean_case()
        c["events"].append(ev("unsupported_mechanism", "doc-secret", mechanism="mmap", op="read"))
        self.assert_not_sat(obs(c), "UNSUPPORTED_MECHANISM", [OPEN, READ, WRITE])
        self.assertEqual(get(obs(c), ATTEMPT), (A.SAT, None))

    def test_unsupported_mechanism_without_resource(self):
        c = clean_case()
        c["events"].append(ev("unsupported_mechanism", None, mechanism="io_uring_setup"))
        self.assert_not_sat(obs(c), "UNSUPPORTED_MECHANISM")

    def test_unsupported_mechanism_outside_policy_is_ignored(self):
        c = clean_case()
        c["events"].append(ev("unsupported_mechanism", "/usr/lib/libc.so.6", mechanism="mmap", op="read"))
        c["events"].append(ev("unsupported_mechanism", "doc-public", mechanism="mmap", op="read"))
        self.assertEqual(get(obs(c), READ), (A.SAT, None))

    def test_unsupported_mechanism_does_not_erase_violation(self):
        c = clean_case()
        c["events"].append(ev("file_read", "doc-secret", n=32))
        c["events"].append(ev("unsupported_mechanism", None, mechanism="io_uring_setup"))
        self.assertEqual(get(obs(c), READ), (A.VIO, None))

    def test_o_path_open_is_not_a_readable_handle(self):
        c = clean_case()
        c["events"].append(ev("file_open", "doc-secret", access="none"))
        self.assertEqual(get(obs(c), OPEN), (A.SAT, None))

    def test_missing_capability_is_not_evaluated(self):
        c = clean_case()
        c["receipt"]["capabilities"] = ["fs", "net", "proc"]
        self.assertEqual(get(obs(c), HEAP), (A.NOT, "REQUIRED_CAPABILITY_UNAVAILABLE"))

    def test_missing_case_produces_no_rows(self):
        # Mukoz HOLDs on the absent rows of the declared external properties (docs/07; e2e.sh checks it).
        t = table(run(obs(clean_case("rnd-00000"))))
        self.assertFalse(any(k[0] == "rnd-00001" for k in t))


class ViolationSurvivesLoss(unittest.TestCase):
    """7.5: log loss after a valid counterexample does not erase it."""

    def violating(self):
        c = clean_case()
        c["events"].append(ev("file_read", "doc-secret", n=32))
        return c

    def test_with_gap(self):
        c = self.violating()
        c["receipt"]["gaps"] = [{"from": 20, "to": 99}]
        c["receipt"]["lost_events"] = 7
        self.assertEqual(get(obs(c), READ), (A.VIO, None))

    def test_with_end_missing(self):
        c = self.violating()
        c["receipt"]["end"] = None
        self.assertEqual(get(obs(c), READ), (A.VIO, None))

    def test_with_unresolved_event_after(self):
        c = self.violating()
        c["events"].append(ev("file_read", None, n=32))
        self.assertEqual(get(obs(c), READ), (A.VIO, None))

    def test_fill_keeps_violation_over_inconclusive_part(self):
        c = self.violating()
        c["events"].append(ev("net_connect", None))
        self.assertEqual(get(obs(c), ATTEMPT)[0], A.INC)
        self.assertEqual(get(obs(c), "effects.no_forbidden"), (A.VIO, None))


class SutOutput(unittest.TestCase):
    """7.5: PASS or a fake JSON report in the SUT output is never promoted to a verdict."""

    FAKE = json.dumps({"schema": "mukoz.external_claims/0", "claims": [{"eval": "SATISFIED_IN_SCOPE"}]})

    def sut_said(self, e):
        e["evidence_origin"] = "sut_output"
        e["detail"] = {"text": "PASS " + self.FAKE}
        return e

    def test_fake_pass_does_not_fill_missing_monitoring(self):
        c = clean_case()
        c["receipt"]["start"] = None
        c["events"].append(self.sut_said(ev("file_read", "doc-public", n=1)))
        self.assertEqual(get(obs(c), READ), (A.INC, "MONITORING_START_MISSING"))

    def test_fake_pass_does_not_hide_violation(self):
        c = clean_case()
        c["events"].append(ev("file_read", "doc-secret", n=32))
        c["events"].append(self.sut_said(ev("file_read", "doc-secret", ok=False, n=-1)))
        self.assertEqual(get(obs(c), READ), (A.VIO, None))

    def test_sut_reported_violation_is_not_evidence(self):
        c = clean_case()
        c["events"].append(self.sut_said(ev("file_read", "doc-secret", n=32)))
        self.assertEqual(get(obs(c), READ), (A.SAT, None))

    def test_supervisor_events_are_out_of_scope(self):
        c = clean_case()
        e = ev("net_connect", "10.0.0.53:53")
        e["subject"]["role"] = "supervisor"
        c["events"].append(e)
        self.assertEqual(get(obs(c), ATTEMPT), (A.SAT, None))

    def test_no_input_field_is_copied_into_an_eval(self):
        c = clean_case()
        c["receipt"]["start"] = None
        c["events"].append(self.sut_said(ev("file_read", "doc-public", n=1)))
        for claim in run(obs(c))["claims"]:
            self.assertIn(claim["eval"], {A.SAT, A.VIO, A.INC, A.NOT})
            self.assertNotEqual(claim["eval"], A.SAT)


class EmptyScope(unittest.TestCase):
    """7.5: an empty scope is not success."""

    def test_no_request_reached(self):
        c = clean_case()
        c["requests_sent"] = 0
        for p in BINDING["properties"]:
            self.assertEqual(get(obs(c), p), (A.INC, "VACUOUS_CASE"), p)

    def test_no_cases_gives_no_claims(self):
        # Mukoz then HOLDs on missing data or VACUOUS_SCOPE; it never sees a SATISFIED row (e2e.sh).
        self.assertEqual(run(obs())["claims"], [])


class Other(unittest.TestCase):
    def test_forbidden_connect_attempt_and_success(self):
        c = clean_case()
        c["events"].append(ev("net_connect", "10.77.0.3:8080", ok=False))
        self.assertEqual(get(obs(c), ATTEMPT), (A.VIO, None))
        self.assertEqual(get(obs(c), CONNECT), (A.SAT, None))
        c["events"].append(ev("net_connect", "10.77.0.3:8080"))
        self.assertEqual(get(obs(c), CONNECT), (A.VIO, None))

    def test_nonblocking_connect_is_not_success(self):
        # EINPROGRESS, then poll/getsockopt (not followed): an attempt, never a satisfied connect.
        c = clean_case()
        e = ev("net_connect", "10.77.0.3:9090", ok=False, result={"success": False, "errno": "EINPROGRESS"})
        e["stage"] = "attempted"
        c["events"].append(e)
        self.assertEqual(get(obs(c), ATTEMPT), (A.VIO, None))
        self.assertEqual(get(obs(c), CONNECT), (A.INC, "OUTCOME_UNOBSERVED"))
        e["resource_id"] = "10.77.0.1:8080"
        self.assertEqual(get(obs(c), CONNECT), (A.SAT, None))

    def test_unauthorized_exec(self):
        c = clean_case()
        c["events"].append(ev("process_exec", "/bin/sh"))
        self.assertEqual(get(obs(c), EXEC), (A.VIO, None))

    def test_heap_error(self):
        c = clean_case()
        e = ev("mem_error", None, detail={"kind": "InvalidWrite"})
        e["evidence_origin"] = "memcheck"
        c["events"].append(e)
        self.assertEqual(get(obs(c), HEAP), (A.VIO, None))

    def test_leak_is_not_heap_violation(self):
        c = clean_case()
        e = ev("mem_error", None, detail={"kind": "Leak_DefinitelyLost"})
        e["evidence_origin"] = "memcheck"
        c["events"].append(e)
        self.assertEqual(get(obs(c), HEAP), (A.SAT, None))

    def test_region_not_evaluated_without_region_capability(self):
        # Memcheck present and silent: the logical-region property is still not evaluated (F12).
        b = copy.deepcopy(BINDING)
        b["properties"][REGION] = "mem.region.read_authorized"
        c = clean_case()
        self.assertEqual(get(obs(c, binding_raw=json.dumps(b).encode()), REGION, binding=b),
                         (A.NOT, "REQUIRED_CAPABILITY_UNAVAILABLE"))
        self.assertEqual(get(obs(c, binding_raw=json.dumps(b).encode()), HEAP, binding=b), (A.SAT, None))

    def test_region_read_with_region_capability(self):
        b = copy.deepcopy(BINDING)
        b["properties"][REGION] = "mem.region.read_authorized"
        braw = json.dumps(b).encode()
        c = clean_case()
        c["receipt"]["capabilities"].append("region")
        e = ev("mem_region_read", None, detail={"region": "token", "authorized": True})
        e["evidence_origin"] = "memcheck"
        c["events"].append(e)
        self.assertEqual(get(obs(c, binding_raw=braw), REGION, binding=b), (A.SAT, None))
        c["events"][-1]["detail"]["authorized"] = False
        self.assertEqual(get(obs(c, binding_raw=braw), REGION, binding=b), (A.VIO, None))


class Malformed(unittest.TestCase):
    """Malformed input exits 2 and writes nothing."""

    def cli(self, o, binding=BINDING, ctx=CTX):
        with tempfile.TemporaryDirectory() as d:
            b, f, out = Path(d, "b.json"), Path(d, "o.json"), Path(d, "out.json")
            b.write_text(binding if isinstance(binding, str) else json.dumps(binding))
            f.write_text(o if isinstance(o, str) else json.dumps(o))
            r = subprocess.run([sys.executable, str(SRC), "--binding", str(b), "--observations", str(f),
                                "--subject-context", ctx, "--independence", "independent", "-o", str(out)],
                               capture_output=True)
            return r.returncode, out.exists()

    def test_ok(self):
        o = obs(clean_case(), binding_raw=json.dumps(BINDING).encode())
        self.assertEqual(self.cli(o), (0, True))

    def test_cases(self):
        good = obs(clean_case())
        bad = []
        x = copy.deepcopy(good); x["cases"][0]["receipt"].pop("gaps"); bad.append(x)
        x = copy.deepcopy(good); x["cases"][0]["events"][0]["result"]["bytes"] = "32"; bad.append(x)
        x = copy.deepcopy(good); x["cases"][0]["events"][0]["result"]["success"] = 1; bad.append(x)
        x = copy.deepcopy(good); x["cases"][0]["extra"] = 1; bad.append(x)
        x = copy.deepcopy(good); x["cases"].append(clean_case()); bad.append(x)
        x = copy.deepcopy(good); x["schema"] = "secobs.observations/1"; bad.append(x)
        bad.append('{"schema": "secobs.observations/0", "schema": "x"}')
        bad.append("[]")
        for i, o in enumerate(bad):
            self.assertEqual(self.cli(o), (2, False), i)
        self.assertEqual(self.cli(good, ctx="AB" * 32), (2, False))
        b = copy.deepcopy(BINDING); b["fills"] = {"effects.no_forbidden": ["external.undeclared"]}
        self.assertEqual(self.cli(good, binding=b), (2, False))
        b = copy.deepcopy(BINDING); b["properties"]["internal.x"] = "fs.no_forbidden_read"
        self.assertEqual(self.cli(good, binding=b), (2, False))


if __name__ == "__main__":
    unittest.main()
