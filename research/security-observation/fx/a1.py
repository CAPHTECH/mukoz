#!/usr/bin/env python3
"""WP2: the A0 (API) and A1 (External) evaluators (DESIGN.md 4.1, 9.1), and their check against the
ground truth of the same runs.

usage: a1.py <runs-dir>        exit 0 iff no false violation, no false success, and every property the
                               condition fully observes is decided exactly (see check())
       (as a module: judge(contract, case, run_dir, "A0"|"A1") -> {property: (eval, reason)})

What each condition observes:
  A0  the client's responses (status, body)
  A1  A0 + SUT stdout/stderr + the fixture manifest before and after the run (size, mode, a/m/ctime,
      sha256 after) + the receivers' accept log

Rules (fixed before the A0/A1 runs were looked at; [R] where an inference is involved):
  FUNC.*        responses against the contract's accepted outcomes. A0 cannot see a delivery to a receiver,
                so a matching response that needs one is INCONCLUSIVE in A0.
  SEC.DATA.*    registered channel: the response body (pilot contracts). A canary of a resource forbidden
                to the case's principal in a body -> VIOLATED; none, with every response received -> SATISFIED.
  SEC.FS.NO_FORBIDDEN_READ, SEC.AUTH.*
                canary of a forbidden resource in a response (A1: or in stdout/stderr) -> VIOLATED [R: the SUT
                had no other source of those bytes; run.sh records that no artifact contains a canary].
                A1 only: else, a forbidden resource's atime changed -> INCONCLUSIVE (METADATA_ONLY: the time
                shows an access, not its bytes or subject, 9.1); else, on a strictatime mount -> SATISFIED
                [R: every read path updates atime under strictatime]; on any other mount -> NOT_EVALUATED.
  SEC.FS.NO_FORBIDDEN_OPEN
                canary inference -> VIOLATED; A1: atime changed -> INCONCLUSIVE; else NOT_EVALUATED (an open
                without a read leaves no trace in the manifest).
  SEC.NET.NO_FORBIDDEN_CONNECT
                A1: an accept at a receiver outside net_allowed -> VIOLATED; receivers ran -> SATISFIED
                [R: on the isolated test network only the receivers listen]. A0: NOT_EVALUATED.
  SEC.NET.NO_FORBIDDEN_ATTEMPT
                A1: such an accept -> VIOLATED (an established connection was attempted); else NOT_EVALUATED
                (a refused attempt reaches no receiver).
  MEM.*         NOT_EVALUATED.
Incomplete evidence (a request without a response; A1: a manifest, stdout/stderr or the receiver log
missing) makes every verdict that needs it INCONCLUSIVE, never SATISFIED.
"""
import base64
import importlib.util
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
SAT, VIO, INC, NOT = "SATISFIED_IN_SCOPE", "VIOLATED", "INCONCLUSIVE", "NOT_EVALUATED"
NOCAP = "REQUIRED_CAPABILITY_UNAVAILABLE"


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def _json(p):
    try:
        return json.loads(p.read_text())
    except (OSError, ValueError):
        return None


def _match(c, alt, r, delivered):
    """True / False, or None when the outcome needs a delivery this condition cannot see."""
    if "error" in r or r["status"] != alt["status"]:
        return False
    body = base64.b64decode(r["body_b64"])
    if "file" in alt and body != (c["files"][alt["file"]] + alt.get("suffix", "")).encode():
        return False
    if "text" in alt and body != alt["text"].encode():
        return False
    if "delivered" in alt:
        if delivered is None:
            return None
        if delivered[alt["delivered"]] == 0:
            return False
        delivered[alt["delivered"]] -= 1
    return True


def judge(c, case, d, cond):
    d = Path(d)
    a1 = cond == "A1"
    b = c.get("binding", {"resources": [], "allow": [], "net_allowed": []})
    allowed = {a["resource"] for a in b["allow"] if a["principal"] == case["principal"] and "read" in a["ops"]}
    forbidden = set(b["resources"]) - allowed
    rj = _json(d / "responses.json")
    resps = rj["responses"] if rj else []
    a0_ok = len(resps) == len(case["requests"]) and all("error" not in r for r in resps)
    bodies = [base64.b64decode(r["body_b64"]) for r in resps if "body_b64" in r]
    before, after = _json(d / "manifest.before.json"), _json(d / "manifest.after.json")
    logs = [(d / f).read_bytes() for f in ("stdout.txt", "stderr.txt") if (d / f).exists()]
    rcv_ran = (d / "receiver.ready").exists() and "READY" in (d / "receiver.ready").read_text()
    rcv = [ln.split() for ln in (d / "receiver.txt").read_text().splitlines()] if (d / "receiver.txt").exists() else []
    a1_ok = a0_ok and before is not None and after is not None and len(logs) == 2 and rcv_ran

    canaries = {r: c["canaries"][r] for r in forbidden if r in c.get("canaries", {})}
    in_resp = any(k.encode() in x for k in canaries.values() for x in bodies)
    inferred = in_resp or (a1 and any(k.encode() in x for k in canaries.values() for x in logs))
    touched = a1_ok and any(r in before["files"] and r in after["files"]
                            and before["files"][r]["atime_ns"] != after["files"][r]["atime_ns"] for r in forbidden)
    strict = a1_ok and not any(o in (after["mount_options"] or "").split(",") for o in ("relatime", "noatime", "lazytime"))
    forbidden_accept = any(x[0] not in b["net_allowed"] for x in rcv)

    out = {}
    for p in c["properties"]:
        if p.startswith("FUNC."):
            delivered = Counter(int(x[2].split("=")[1]) for x in rcv if x[0] in b["net_allowed"]) if a1 and rcv_ran else None
            if len(resps) != len(case["accept"]) or not a0_ok:
                out[p] = (INC, "RESPONSE_MISSING")
                continue
            res = [[_match(c, alt, r, delivered) for alt in alts] for r, alts in zip(resps, case["accept"])]
            if all(any(x is True for x in r) for r in res):
                out[p] = (SAT, None)
            elif all(any(x is not False for x in r) for r in res):
                out[p] = (INC, "EXTERNAL_STATE_UNOBSERVED")
            else:
                out[p] = (VIO, None)
        elif p == "SEC.DATA.NO_SECRET_OUTPUT":
            out[p] = (VIO, None) if in_resp else (SAT, None) if a0_ok else (INC, "RESPONSE_MISSING")
        elif p in ("SEC.FS.NO_FORBIDDEN_READ", "SEC.AUTH.RESOURCE_ACCESS"):
            if inferred:
                out[p] = (VIO, None)
            elif not a1:
                out[p] = (NOT, NOCAP)
            elif not a1_ok:
                out[p] = (INC, "EXTERNAL_STATE_INCOMPLETE")
            elif touched:
                out[p] = (INC, "METADATA_ONLY")
            elif strict:
                out[p] = (SAT, None)
            else:
                out[p] = (NOT, "ATIME_NOT_STRICT")
        elif p == "SEC.FS.NO_FORBIDDEN_OPEN":
            if inferred:
                out[p] = (VIO, None)
            elif a1 and not a1_ok:
                out[p] = (INC, "EXTERNAL_STATE_INCOMPLETE")
            elif a1 and touched:
                out[p] = (INC, "METADATA_ONLY")
            else:
                out[p] = (NOT, NOCAP)
        elif p == "SEC.NET.NO_FORBIDDEN_CONNECT":
            out[p] = (NOT, NOCAP) if not a1 else (VIO, None) if forbidden_accept else (SAT, None) if a1_ok else (INC, "EXTERNAL_STATE_INCOMPLETE")
        elif p == "SEC.NET.NO_FORBIDDEN_ATTEMPT":
            out[p] = (VIO, None) if a1 and forbidden_accept else (NOT, NOCAP)
        else:
            out[p] = (NOT, NOCAP)
    return out


# Properties the condition observes completely: its verdict must equal the ground truth, not only be sound.
EXACT = {"A0": {"SEC.DATA.NO_SECRET_OUTPUT"}, "A1": {"SEC.DATA.NO_SECRET_OUTPUT", "SEC.NET.NO_FORBIDDEN_CONNECT"}}


def check(root, quiet=False):
    W = _load("witness", HERE / "witness.py")
    bad = n = 0
    detect = defaultdict(Counter)
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
                # Memory has no witness in these runs (no gdb): its truth is the label, witnessed natively (A9).
                truth = seen | {p for p in case["labels"][v] if p.startswith("MEM.")}
                if truth != set(case["labels"][v]):
                    problems.append(f"ground truth {sorted(truth)} labeled {sorted(case['labels'][v])}")
                for cond in ("A0", "A1"):
                    for p, (ev, reason) in judge(c, case, d, cond).items():
                        violated = (not func_ok) if p.startswith("FUNC.") else p in truth
                        if ev == VIO and not violated:
                            problems.append(f"{cond} {p}: false VIOLATED")
                        if ev == SAT and violated:
                            problems.append(f"{cond} {p}: false SATISFIED")
                        if (p in EXACT[cond] or (p.startswith("FUNC.") and (cond == "A1" or "delivered" not in json.dumps(case["accept"])))) \
                                and ev != (VIO if violated else SAT):
                            problems.append(f"{cond} {p}: {ev}({reason}) where it observes completely")
                        if violated and not p.startswith("FUNC."):
                            detect[(c["scenario"], cond)][ev if ev != INC else f"{ev}({reason})"] += 1
                if problems:
                    bad += 1
                    if not quiet:
                        print(f"BAD {case['id']} {v}: " + " | ".join(problems))
    if not quiet:
        print("verdicts on the violated (case, property) pairs:")
        for (sc, cond), k in sorted(detect.items()):
            print(f"  {sc} {cond}: {dict(k)}")
        print(f"{n - bad}/{n} runs clean")
    return 1 if bad or not n else 0


if __name__ == "__main__":
    sys.exit(check(sys.argv[1]))
