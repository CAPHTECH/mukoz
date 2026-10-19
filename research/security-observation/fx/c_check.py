#!/usr/bin/env python3
"""WP4: check the pilot runs under Valgrind (FX_MODE=c0 Nulgrind, c Memcheck; both under strace).

usage: c_check.py c0|c <runs-dir>      exit 0 iff every run is as expected

Per run:
1. The artifact still behaves (11.4): FUNC and the file/network ground truth (witness.py, without the
   memory properties: no gdb under Valgrind) equal the native labels.
2. The verdicts, from strace (normalize/strace.py, SUT scope from /usr/bin/valgrind) and, for c, the
   Memcheck XML (normalize/memcheck.py), through adapter/adapter.py:
     fs / net properties      VIOLATED iff labeled, else SATISFIED_IN_SCOPE (as B)
     mem.heap.addressable     c: VIOLATED iff MEM.HEAP.ADDRESSABLE is labeled, else SATISFIED_IN_SCOPE;
                              c0: NOT_EVALUATED (no memory checking)
     mem.region.read_authorized (F12)  NOT_EVALUATED in both: Memcheck cannot see a logical region
                              inside a valid allocation, and the claim must not become SATISFIED.
"""
import copy
import hashlib
import importlib.util
import json
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


S = load("strace_norm", ROOT / "normalize" / "strace.py")
M = load("memcheck_norm", ROOT / "normalize" / "memcheck.py")
A = load("adapter", ROOT / "adapter" / "adapter.py")
W = load("witness", HERE / "witness.py")
HEAP, REGION = "external.mem.heap.addressable", "external.mem.region.read_authorized"
LABEL = {"external.sec.fs.no_forbidden_open": "SEC.FS.NO_FORBIDDEN_OPEN", "external.sec.fs.no_forbidden_read": "SEC.FS.NO_FORBIDDEN_READ",
         "external.sec.net.no_forbidden_attempt": "SEC.NET.NO_FORBIDDEN_ATTEMPT",
         "external.sec.net.no_forbidden_connect": "SEC.NET.NO_FORBIDDEN_CONNECT", HEAP: "MEM.HEAP.ADDRESSABLE", REGION: "MEM.REGION.READ_AUTHORIZED"}
VG_EXEC = ["/usr/bin/valgrind", "/usr/bin/valgrind.bin", "/usr/libexec/valgrind/none-amd64-linux",
           "/usr/libexec/valgrind/memcheck-amd64-linux"]


def binding_of(c, mode="c"):
    b = copy.deepcopy(c.get("binding") or {"schema": "secobs.binding/0", "resources": [], "allow": [], "net_allowed": [],
                                           "exec_allowed": [], "properties": {}})
    if mode == "b":
        return b
    b["exec_allowed"] += VG_EXEC
    b["properties"][HEAP] = "mem.heap.addressable"
    if "MEM.REGION.READ_AUTHORIZED" in c["properties"]:
        b["properties"][REGION] = "mem.region.read_authorized"
    return b


def claims(mode, c, case, d):
    """The OS (and, for c, Memcheck) verdicts of one run: {contract property: (eval, reason)} for every
    property the binding carries. mode b: strace under /sut/; c0, c: strace under Valgrind (+ Memcheck XML)."""
    d = Path(d)
    b = binding_of(c, mode)
    if not b["properties"]:
        return {}   # B on F10/F12: strace carries no property of these contracts
    braw = json.dumps(b).encode()
    events, receipt = S.parse((d / "strace.txt").read_text(errors="replace"), ["/srv/fx/"],
                              "/sut/" if mode == "b" else "/usr/bin/valgrind", controller_end(d))
    if mode == "c":
        me, mr = M.parse((d / "vg.xml").read_text(errors="replace"))
        events, receipt = events + me, M.merge(receipt, mr)
    o = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(braw).hexdigest(),
         "cases": [{"case_id": case["id"], "principal": case["principal"], "requests_sent": len(case["requests"]),
                    "receipt": receipt, "events": events}]}
    out, _ = A.convert(braw, b, json.dumps(o).encode(), o, "0" * 64, "independent")
    return {LABEL[x["property"]]: (x["eval"], x["reason"]) for x in out["claims"] if x["property"] in b["properties"]}


def expect(mode, prop, labels):
    if prop == REGION or (prop == HEAP and mode == "c0"):
        return A.NOT
    return A.VIO if LABEL[prop] in labels else A.SAT


def controller_end(d):
    t = (d / "sbx.status").read_text()
    return "drained" if "all_exited" in t else "grace_expired" if "grace_expired" in t else "unknown"


def main(mode, root):
    bad = n = 0
    tally = Counter()
    for cf in sorted((HERE / "contract").glob("F*.json")):
        c = json.loads(cf.read_text())
        cw = dict(c, properties=[p for p in c["properties"] if not p.startswith("MEM.")])
        for v in ("V", "R", "H"):
            for case in c["cases"]:
                d = Path(root) / c["scenario"] / v / case["id"]
                if not d.exists():
                    continue
                n += 1
                problems = []
                seen, func_ok, prob = W.judge(cw, case, v, d)
                problems += prob
                want_seen = {p for p in case["labels"][v] if not p.startswith("MEM.")}
                if seen != want_seen:
                    problems.append(f"ground truth {sorted(seen)} labeled {sorted(want_seen)}")
                if not func_ok and not (v == "V" and case["class"] == "forbidden"):
                    problems.append("FUNC failed")
                rev = {v_: k for k, v_ in LABEL.items()}
                for lp, (ev, reason) in claims(mode, c, case, d).items():
                    p = rev[lp]
                    want = expect(mode, p, case["labels"][v])
                    tally[(p.split(".", 1)[1], ev)] += 1
                    if ev != want:
                        problems.append(f"{p.split('.', 2)[-1]}={ev}({reason}) want {want}")
                bad += bool(problems)
                if problems:
                    print(f"BAD {case['id']} {v}: " + " | ".join(problems))
    for (p, ev), k in sorted(tally.items()):
        print(f"  {p} {ev}: {k}")
    print(f"{n - bad}/{n} runs as expected")
    return 1 if bad or not n else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
