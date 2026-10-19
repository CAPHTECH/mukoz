#!/usr/bin/env python3
"""Pass the pilot runs' strace records through B (normalize/strace.py, then adapter/adapter.py)
and compare B's verdicts with the contract labels. Public cases only: this is calibration (12.1).

usage: b_check.py <runs-dir>      prints one line per run; exit 0 iff every verdict is as expected

Expected: VIOLATED where the label names the property, otherwise SATISFIED_IN_SCOPE. Anything else
(INCONCLUSIVE with its reason) is reported, not hidden: it is what B cannot decide on these artifacts.
"""
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
A = load("adapter", ROOT / "adapter" / "adapter.py")
LABEL = {"external.sec.fs.no_forbidden_open": "SEC.FS.NO_FORBIDDEN_OPEN", "external.sec.fs.no_forbidden_read": "SEC.FS.NO_FORBIDDEN_READ",
         "external.sec.net.no_forbidden_attempt": "SEC.NET.NO_FORBIDDEN_ATTEMPT",
         "external.sec.net.no_forbidden_connect": "SEC.NET.NO_FORBIDDEN_CONNECT"}


def controller_end(d):
    t = (d / "sbx.status").read_text()
    return "drained" if "all_exited" in t else "grace_expired" if "grace_expired" in t else "unknown"


def main(root):
    bad = n = 0
    tally = Counter()
    for cf in sorted((HERE / "contract").glob("F*.json")):
        c = json.loads(cf.read_text())
        if "binding" not in c:
            continue
        braw = json.dumps(c["binding"]).encode()
        for v in ("V", "R", "H"):
            for case in c["cases"]:
                d = Path(root) / c["scenario"] / v / case["id"]
                if not (d / "strace.txt").exists():
                    continue
                n += 1
                events, receipt = S.parse((d / "strace.txt").read_text(errors="replace"), ["/srv/fx/"], "/sut/", controller_end(d))
                o = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(braw).hexdigest(),
                     "cases": [{"case_id": case["id"], "principal": case["principal"], "requests_sent": len(case["requests"]),
                                "receipt": receipt, "events": events}]}
                out, _ = A.convert(braw, c["binding"], json.dumps(o).encode(), o, "0" * 64, "independent")
                got = {x["property"]: (x["eval"], x["reason"]) for x in out["claims"] if x["property"] in LABEL}
                wrong = []
                for p, (ev, reason) in sorted(got.items()):
                    want = A.VIO if LABEL[p] in case["labels"][v] else A.SAT
                    tally[(c["scenario"], ev)] += 1
                    if ev != want:
                        wrong.append(f"{p.split('.', 2)[-1]}={ev}({reason}) want {want}")
                bad += bool(wrong)
                gaps = sorted({g["reason"] for g in receipt["gaps"]})
                print(f"{'ok ' if not wrong else 'BAD'} {case['id']} {v}: end={receipt['end']} gaps={gaps} " + "; ".join(wrong))
    for (sc, ev), k in sorted(tally.items()):
        print(f"  {sc} {ev}: {k}")
    print(f"{n - bad}/{n} runs as expected")
    return 1 if bad or not n else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
