"""Judge todo submissions: hidden oracle (fresh seed, 300 sessions) + Mukoz on the final file.
usage: python3 eval/todo/judge_exp.py <exp-dir> <mukoz> <run-dir>...   -> appends <exp-dir>/results.jsonl"""
import glob, json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import oracle_todo  # noqa: E402

exp, mukoz = sys.argv[1], sys.argv[2]
for d in sys.argv[3:]:
    d = d if os.path.isabs(d) else os.path.join(exp, d)
    name = os.path.basename(d.rstrip("/"))
    sol = os.path.join(d, "solution.bin")
    rec = {"run": name, "condition": name[1]}
    if not os.path.exists(sol):
        rec["oracle"] = "NO_SUBMISSION"
    else:
        j = oracle_todo.judge(sol, 300)
        out = subprocess.run([mukoz, "check", os.path.join(HERE, "suite.toml"), "--artifact", sol, "--store", sol + ".judge-store"],
                             capture_output=True, text=True, timeout=1200).stdout
        try:
            a = json.loads(out)["data"]["assessment"]
            mz, reasons = a["admission"], a["reasons"][:3]
        except Exception:
            mz, reasons = "ERROR", []
        rec.update({"oracle": j["verdict"], "oracle_seed": j["seed"], "oracle_failed_sessions": j["failed_sessions"],
                    "oracle_commands": j["commands"], "oracle_first_failures": j["first_failures"][:2],
                    "mukoz_on_final": mz, "mukoz_reasons": reasons, "bytes": os.path.getsize(sol)})
    if name[1] == "A":
        rec["mukoz_check_runs"] = len(glob.glob(os.path.join(d, ".mukoz", "items", "run-*.json")))
    print(json.dumps(rec))
    with open(os.path.join(exp, "results.jsonl"), "a") as f:
        f.write(json.dumps(rec) + "\n")
