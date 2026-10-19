#!/usr/bin/env python3
"""E1 plan (DESIGN.md 11.2): every block = scenario x implementation x variant x case x seed, its five
conditions (A0, A1, B, C0, C) in an order from a fixed seed, blocks in a shuffled order, dealt to workers.

usage: e1plan.py <contract-dir> <out-prefix> <workers> [rng-seed]    writes <out-prefix>.<k> for k in 0..workers-1
Condition orders cycle through all 120 permutations in a shuffled sequence, so that each condition
takes each position equally often over every 120 blocks. A block stays on one worker, its conditions
run one after another (12.3: 1 block 内逐次); blocks on different workers run in parallel.
"""
import itertools
import json
import random
import sys
from pathlib import Path

CONDS = ("a0", "a1", "b", "c0", "c")
IMPLS = (1, 2)
SEEDS = (1, 2, 3)


def main(cdir, prefix, workers, rng_seed="20261019"):
    rng = random.Random(int(rng_seed))
    blocks = []
    for f in sorted(Path(cdir).glob("F*.json")):
        c = json.loads(f.read_text())
        for impl in c.get("implementations", IMPLS):
            for v in "VRH":
                for case in c["cases"]:
                    for seed in SEEDS:
                        blocks.append((c["scenario"], v, case["id"], impl, seed))
    rng.shuffle(blocks)
    perms = list(itertools.permutations(CONDS))
    order = []
    while len(order) < len(blocks):
        p = perms[:]
        rng.shuffle(p)
        order += p
    shards = [[] for _ in range(int(workers))]
    for i, (b, p) in enumerate(zip(blocks, order)):
        sc, v, cs, impl, seed = b
        shards[i % len(shards)] += [f"{sc} {v} {cs} {cond} {impl} {seed}" for cond in p]
    for k, lines in enumerate(shards):
        Path(f"{prefix}.{k}").write_text("\n".join(lines) + "\n")
    print(f"{len(blocks)} blocks, {len(blocks) * len(CONDS)} runs, {len(shards)} workers")
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
