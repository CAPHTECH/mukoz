#!/usr/bin/env python3
"""Run self-check stage 3 with a given mukoz binary and print one line per suite.
usage: run.py <mukoz> <store> [--json out.json]"""
import glob, json, os, subprocess, sys
here = os.path.dirname(os.path.abspath(__file__))
mukoz, store = sys.argv[1], sys.argv[2]
EXPECT = {"copy": ("error", "UNRESOLVED_DEPENDENCY")}
results = []
for suite in sorted(glob.glob(os.path.join(here, "*/suite.*.toml"))):
    group = os.path.basename(os.path.dirname(suite))
    out = subprocess.run([mukoz, "check", suite, "--store", store, "--policy", os.path.join(here, "policy.toml")], capture_output=True)
    v = json.loads(out.stdout)
    rel = os.path.relpath(suite, here)
    if v["data"] is None:
        got = ("error", v["errors"][0]["code"])
        r = {"suite": rel, "outcome": got[1], "ok": EXPECT.get(group) == got}
    else:
        a = v["data"]["assessment"]
        diff = [c for c in v["data"]["claims"] if c["property"].startswith("differential.")]
        # Pass: every claim satisfied; the only reason not to accept is that the checker is this
        # same version (independence = self, docs/06 6.9).
        claims_ok = all(c["evaluation"] == "SATISFIED_IN_SCOPE" for c in v["data"]["claims"])
        only_self = all(r.startswith("SELF_CHECK_ONLY") for r in a["reasons"])
        r = {"suite": rel, "outcome": a["admission"], "reasons": a["reasons"][:4], "cases": a["scope"]["cases_completed"],
             "independence": a["independence"]["value"], "differential": diff[0]["evaluation"] if diff else None,
             "ok": group not in EXPECT and claims_ok and (a["admission"] == "ACCEPT_WITHIN_SCOPE" or (a["admission"] == "HOLD" and only_self))}
    results.append(r)
    print(("ok  " if r["ok"] else "BAD ") + rel, r["outcome"], r.get("cases", ""), r.get("differential") or "", "" if r["ok"] else r.get("reasons", ""))
if "--json" in sys.argv:
    json.dump(results, open(sys.argv[sys.argv.index("--json") + 1], "w"), indent=1)
sys.exit(0 if all(r["ok"] for r in results) else 1)
