#!/usr/bin/env python3
"""E1 aggregation (DESIGN.md 11.2, 13.1): verdicts of the five conditions per block against the ground truth
of the same runs.

usage: e1.py <runs-dir> <contracts-dir> [--json out.json]
  runs-dir       run.sh plan output: i<impl>-s<seed>-<cond>/<sc>/<v>/<case>
  contracts-dir  seed.py output: s<seed>/F*.json

Per run, ground truth: files and connections from witness.py on that run (fanotify, VFS, SYN capture,
receivers); memory from the gdb witness of the block's A0 and A1 runs (the only runs without ptrace; [R]
the fixtures are deterministic per input, so the block shares one memory truth; A0/A1 disagreeing is
reported). FUNC truth: witness.py's response check on that run.
Per condition, the verdict of each contract property:
  A0, A1   a1.judge on the run (the frozen WP2 rules)
  B        A1 rules on the B run combined with the strace verdicts (c_check.claims "b")
  C0, C    A1 rules combined with strace (+ Memcheck for C) verdicts (c_check.claims "c0" / "c")
  combined: VIOLATED if either says so, else SATISFIED_IN_SCOPE if either does, else the first hold.
  A VIOLATED from one side and SATISFIED from the other is counted as a contradiction.
Root cause = (scenario, implementation): the V artifact's one violation (DESIGN.md 13.1). A mode detects
it if some run of that root cause has VIOLATED on a property the ground truth marks violated; stable if
for some case every seed does.
"""
import importlib.util
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


W = load("witness", HERE / "witness.py")
A1 = load("a1", HERE / "a1.py")
CC = load("c_check", HERE / "c_check.py")
SAT, VIO, INC, NOT = A1.SAT, A1.VIO, A1.INC, A1.NOT
CONDS = ("a0", "a1", "b", "c0", "c")
RANK = {VIO: 0, SAT: 1, INC: 2, NOT: 3}
LAST = {}   # the last main()'s results, for test_e1.py


def combine(*vs):
    vs = [v for v in vs if v]
    best = min(vs, key=lambda v: RANK[v[0]])
    contra = any(v[0] == VIO for v in vs) and any(v[0] == SAT for v in vs)
    return best, contra


def timing(d):
    t = {}
    f = d / "timing.txt"
    if f.exists():
        for ln in f.read_text().split("\n"):
            if ln.strip():
                k, v = ln.split()
                t[k] = float(v)
    return t.get("t_end", 0) - t.get("t_start", 0) if "t_end" in t and "t_start" in t else None


def verdicts(cond, c, case, d):
    if cond in ("a0", "a1"):
        return A1.judge(c, case, d, cond.upper()), 0
    ext = A1.judge(c, case, d, "A1")
    try:
        low = CC.claims(cond, c, case, d)
    except (OSError, ValueError) as e:
        low = {p: (INC, f"EVIDENCE_UNREADABLE {type(e).__name__}") for p in c["properties"] if not p.startswith("FUNC.")}
    out, contra = {}, 0
    for p in c["properties"]:
        out[p], k = combine(ext.get(p), low.get(p))
        contra += k
    return out, contra


def main(root, cdir, *rest):
    root, cdir = Path(root), Path(cdir)
    runs = {}
    for d in root.glob("i*-s*-*/F*/*/*"):
        i, s, cond = d.parts[-4].split("-")
        runs[(d.parts[-3], int(i[1:]), d.parts[-2], d.parts[-1], int(s[1:]), cond)] = d
    contracts = {}
    blocks = defaultdict(dict)
    for (sc, impl, v, cs, seed, cond), d in runs.items():
        blocks[(sc, impl, v, cs, seed)][cond] = d
    rows = []
    issues = Counter()
    notes = []
    for (sc, impl, v, cs, seed), conds in sorted(blocks.items()):
        if (seed, sc) not in contracts:
            contracts[(seed, sc)] = json.loads((cdir / f"s{seed}" / f"{sc}.json").read_text())
        c = contracts[(seed, sc)]
        case = next(x for x in c["cases"] if x["id"] == cs)
        cw = dict(c, properties=[p for p in c["properties"] if not p.startswith("MEM.")])
        missing = [k for k in CONDS if k not in conds]
        if missing:
            issues["block missing conditions"] += 1
            notes.append(f"{sc} i{impl} {v} {cs} s{seed}: missing {missing}")
        mem = {}
        for k in ("a0", "a1"):
            if k in conds and any(p.startswith("MEM.") for p in c["properties"]):
                seen, _, prob = W.judge(c, case, v, conds[k])
                if prob:
                    issues["memory witness problem"] += 1
                    notes.append(f"{sc} i{impl} {v} {cs} s{seed} {k}: {prob}")
                mem[k] = {p for p in seen if p.startswith("MEM.")}
        if len({frozenset(x) for x in mem.values()}) > 1:
            issues["A0/A1 memory truth differs"] += 1
            notes.append(f"{sc} i{impl} {v} {cs} s{seed}: memory truth {mem}")
        memtruth = set().union(*mem.values()) if mem else set()
        truths = {}
        for k, d in conds.items():
            seen, func_ok, prob = W.judge(cw, case, v, d)
            if prob:
                issues["witness problem"] += 1
                notes.append(f"{sc} i{impl} {v} {cs} s{seed} {k}: {prob}")
            truth = seen | memtruth
            truths[k] = (frozenset(truth), func_ok)
            if truth != set(case["labels"][v]):
                issues["ground truth differs from label"] += 1
                notes.append(f"{sc} i{impl} {v} {cs} s{seed} {k}: truth {sorted(truth)} label {sorted(case['labels'][v])}")
            ver, contra = verdicts(k, c, case, d)
            if contra:
                issues["contradiction"] += contra
                notes.append(f"{sc} i{impl} {v} {cs} s{seed} {k}: VIOLATED and SATISFIED combined")
            for p, (ev, reason) in ver.items():
                violated = (not func_ok) if p.startswith("FUNC.") else p in truth
                rows.append({"sc": sc, "impl": impl, "v": v, "case": cs, "cls": case["class"], "seed": seed, "cond": k,
                             "prop": p, "violated": violated, "eval": ev, "reason": reason, "time": timing(d)})
        if len(set(truths.values())) > 1:
            issues["behavior differs across conditions (11.4)"] += 1
            notes.append(f"{sc} i{impl} {v} {cs} s{seed}: " + "; ".join(f"{k}={sorted(t)},func={f}" for k, (t, f) in sorted(truths.items())))
    sec = [r for r in rows if not r["prop"].startswith("FUNC.")]
    # false verdicts
    for r in rows:
        if r["eval"] == VIO and not r["violated"]:
            issues[f"false VIOLATED {r['cond']}"] += 1
            notes.append(f"false VIOLATED {r}")
        if r["eval"] == SAT and r["violated"]:
            issues[f"false SATISFIED {r['cond']}"] += 1
            notes.append(f"false SATISFIED {r}")
    # detection per root cause
    rcs = sorted({(r["sc"], r["impl"]) for r in sec if r["violated"]})
    det = {k: set() for k in CONDS}
    stable = {k: set() for k in CONDS}
    tp = defaultdict(lambda: defaultdict(set))
    for r in sec:
        if r["violated"] and r["eval"] == VIO:
            det[r["cond"]].add((r["sc"], r["impl"]))
            tp[(r["cond"], r["sc"], r["impl"], r["case"])][r["prop"]].add(r["seed"])
    for (k, sc, impl, cs), props in tp.items():
        if any(len(s) == 3 for s in props.values()):
            stable[k].add((sc, impl))
    # holds on violated pairs, per condition
    holds = defaultdict(Counter)
    for r in sec:
        if r["violated"]:
            holds[(r["sc"], r["cond"])][r["eval"] if r["eval"] != INC else f"INC({r['reason']})"] += 1
    # cost per condition and paired ratios per block
    times = defaultdict(list)
    per_block = defaultdict(dict)
    for r in rows:
        if r["time"] is not None:
            per_block[(r["sc"], r["impl"], r["v"], r["case"], r["seed"])][r["cond"]] = r["time"]
    for b in per_block.values():
        for k, t in b.items():
            times[k].append(t)
    ratio = {}
    for a, b in (("b", "a1"), ("c", "b"), ("c0", "b"), ("c", "c0")):
        xs = [x[a] / x[b] for x in per_block.values() if a in x and b in x and x[b] > 0]
        ratio[f"{a}/{b}"] = statistics.median(xs) if xs else None

    LAST.clear()
    LAST.update(detected=det, stable=stable, issues=issues, rows=rows)

    def fmt(s):
        return " ".join(f"{sc}/{i}" for sc, i in sorted(s)) or "-"
    print(f"blocks {len(blocks)}, runs {len(runs)}, verdict rows {len(rows)}")
    print(f"root causes with a violation in the ground truth: {fmt(rcs)}")
    for k in CONDS:
        print(f"  {k:3} detected {len(det[k])}/{len(rcs)}: {fmt(det[k])}   stable: {fmt(stable[k])}")
    print(f"  B \\ A1: {fmt(det['b'] - det['a1'])}   A1 \\ B: {fmt(det['a1'] - det['b'])}")
    print(f"  C \\ B: {fmt(det['c'] - det['b'])}   B \\ C: {fmt(det['b'] - det['c'])}   C \\ C0: {fmt(det['c'] - det['c0'])}")
    print("verdicts on violated (run, property) pairs:")
    for (sc, k), cnt in sorted(holds.items(), key=lambda x: (x[0][0], CONDS.index(x[0][1]))):
        print(f"  {sc} {k:3}: {dict(cnt)}")
    print("time per run, median s: " + ", ".join(f"{k} {statistics.median(times[k]):.2f}" for k in CONDS if times[k]))
    print("paired ratio, median: " + ", ".join(f"{k} {v:.2f}" for k, v in ratio.items() if v))
    print("issues: " + (", ".join(f"{k}: {v}" for k, v in sorted(issues.items())) or "none"))
    for n in notes[:40]:
        print("  " + n)
    if len(notes) > 40:
        print(f"  ... {len(notes) - 40} more")
    if "--json" in rest:
        Path(rest[rest.index("--json") + 1]).write_text(json.dumps(
            {"rows": rows, "issues": issues, "notes": notes, "detected": {k: sorted(v) for k, v in det.items()},
             "stable": {k: sorted(v) for k, v in stable.items()}, "root_causes": rcs, "ratio": ratio}, indent=0, default=list))
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
