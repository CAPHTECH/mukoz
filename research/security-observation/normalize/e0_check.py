#!/usr/bin/env python3
"""Run the strace normalizer and the adapter on the recorded E0 and WP0.7 traces.

usage: e0_check.py <runs-dir> [<runs-dir> ...]   exit 0 iff every trace matches both tables

Two tables are checked for each run that has strace.txt and a known calibration item:
1. events: per-fixture-file bytes, files opened and failed attempts equal e0/check.py EXPECT
   (the expectation the ground-truth layers fanotify and VFS-bpftrace matched in E0 and WP0.7);
2. claims: adapter verdicts with e0_binding.json equal VERDICT below.
"""
import hashlib
import importlib.util
import json
import os
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


S = load("strace_norm", Path(os.environ.get("NORMALIZER", HERE / "strace.py")))
A = load("adapter", ROOT / "adapter" / "adapter.py")
EXPECT = load("e0check", ROOT / "e0" / "check.py").EXPECT

FX = ["/mnt/fx/", "/srv/fx/"]
BRAW = (HERE / "e0_binding.json").read_bytes()
BINDING = json.loads(BRAW)
P = {k.rsplit(".", 1)[-1] if "fs." not in k else k.split("fs.")[-1]: k for k in BINDING["properties"]}
SAT, VIO, INC = A.SAT, A.VIO, A.INC
ALL_SAT = {p: SAT for p in BINDING["properties"]}


def verdict(**vio):
    v = dict(ALL_SAT)
    for short, ev in vio.items():
        v[P[short]] = ev
    return v


# Per item, under principal "cal" of e0_binding.json (secret: nothing; rw: read only; decoy, public: read).
VERDICT = {
    "noop": verdict(),
    "fdreuse": verdict(no_forbidden_open=VIO, no_forbidden_read=VIO),
    "fail": verdict(no_forbidden_attempt=VIO),
    "kxfer": verdict(no_forbidden_open=VIO, no_forbidden_read=VIO),
    "child": verdict(no_forbidden_open=VIO, no_forbidden_read=VIO),
    "symlink": verdict(no_forbidden_open=VIO, no_forbidden_read=VIO),
    "restore": verdict(no_forbidden_open=VIO, no_forbidden_write=VIO),
    "late": verdict(no_forbidden_open=VIO, no_forbidden_read=VIO),
    "normal": verdict(),
    "heapover": verdict(),
    "uaf": verdict(),
    "logical": verdict(),
}
# Runs where the detached child is killed before it reads (pid namespace, grace 0): nothing may be SAT.
KILLED = {"ns-strace-late", "sbx-grace0-late"}


def sut_exec(run):
    if run.startswith("sbx-"):
        return "/sut/"
    if run.startswith(("strace-vg-", "ns-vg-")):
        return "/usr/bin/valgrind"
    return "./cal"


def controller_end(d):
    st = d / "sbx.status"
    if not st.exists():
        return None
    t = st.read_text()
    return "drained" if "all_exited" in t else "grace_expired" if "grace_expired" in t else "unknown"


def tables(events):
    by = defaultdict(lambda: [0, 0])
    opened, fails = set(), []

    def fx(r):
        return r is not None and not r.startswith("/") and ":" not in r

    for e in events:
        if e["subject"]["role"] != "sut":
            continue
        k, r, ok, n = e["event_kind"], e["resource_id"], e["result"]["success"], e["result"].get("bytes")
        if k == "file_open":
            if ok and fx(r):
                opened.add(r)
            elif not ok and fx(r):
                fails.append(("openat", r, e["result"].get("errno")))
        elif k in ("file_read", "file_write"):
            if ok and fx(r) and n:
                by[r][0 if k == "file_read" else 1] += n
            elif not ok and fx(r):
                fails.append(("read" if k == "file_read" else "write", r, e["result"].get("errno")))
        elif k == "file_transfer" and ok and n:
            if fx(r):
                by[r][0] += n
            if fx(e.get("dest_resource_id")):
                by[e["dest_resource_id"]][1] += n
        elif k == "net_connect" and not ok:
            fails.append(("connect", r, e["result"].get("errno")))
    return {k: tuple(v) for k, v in by.items() if v != [0, 0]}, opened, fails


def check(d):
    item = d.name.split("-")[-1]
    events, receipt = S.parse((d / "strace.txt").read_text(errors="replace"), FX, sut_exec(d.name), controller_end(d))
    notes = []
    eb, eopen, efail = EXPECT[item]
    if d.name not in KILLED:
        b, o, f = tables(events)
        if (b, o, sorted(f)) != (eb, eopen, sorted(efail)):
            notes.append(f"events bytes={b} opened={sorted(o)} fails={f}")
    case = {"case_id": d.name, "principal": "cal", "requests_sent": 1, "receipt": receipt, "events": events}
    o = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(BRAW).hexdigest(), "cases": [case]}
    out, _ = A.convert(BRAW, BINDING, json.dumps(o).encode(), o, "0" * 64, "independent")
    got = {c["property"]: c["eval"] for c in out["claims"] if c["property"].startswith("external.")}
    reasons = {c["reason"] for c in out["claims"] if c["reason"]}
    if d.name in KILLED:
        if any(v == SAT for v in got.values()):
            notes.append(f"claims SAT after a forced stop: {got}")
    elif got != VERDICT[item]:
        notes.append("claims " + ", ".join(f"{k.split('.', 2)[-1]}={v}" for k, v in got.items() if v != VERDICT[item][k]))
    return notes, receipt, reasons


def main(argv):
    bad = n = 0
    for root in argv:
        for d in sorted(Path(root).iterdir()):
            if not (d / "strace.txt").exists() or d.name.split("-")[-1] not in EXPECT:
                continue
            n += 1
            notes, r, reasons = check(d)
            g = sorted({x["reason"] for x in r["gaps"]})
            tag = "ok " if not notes else "BAD"
            bad += bool(notes)
            print(f"{tag} {root.rstrip('/').split('/')[-1]}/{d.name}: end={r['end']} gaps={g} lost={r['lost_events']} reasons={sorted(reasons)}")
            for x in notes:
                print("      " + x)
    print(f"{n - bad}/{n} runs match")
    return 1 if bad or not n else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
