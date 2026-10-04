"""Summarize results.jsonl: per condition, oracle pass rate, false acceptances."""
import json, sys
from collections import defaultdict

rows = {}
for line in open(sys.argv[1]):
    r = json.loads(line)
    rows[r["run"]] = r  # last judgement wins
by = defaultdict(list)
for r in rows.values():
    by[r["condition"]].append(r)
print("| run | oracle | Mukoz on final | mukoz check runs |")
print("|---|---|---|---|")
for k in sorted(rows):
    r = rows[k]
    print(f"| {k} | {r['oracle']} ({r.get('oracle_failed', '-')} failed) | {r.get('mukoz_on_final', '-')} | {r.get('mukoz_check_runs', '-')} |")
for c, rs in sorted(by.items()):
    ok = sum(r["oracle"] == "PASS" for r in rs)
    fa = sum(r["oracle"] != "PASS" and r.get("mukoz_on_final") == "ACCEPT_WITHIN_SCOPE" for r in rs)
    print(f"\ncondition {c}: {ok}/{len(rs)} pass the hidden oracle; Mukoz ACCEPT but oracle FAIL: {fa}")
