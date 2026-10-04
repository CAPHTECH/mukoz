"""Judge every experiment submission with the hidden oracle (fresh seed, after
submission) and record Mukoz's own verdict on the final file for comparison.

usage: python3 eval/judge_all.py <exp-dir> <mukoz-binary> [task-dirs...]
Writes <exp-dir>/results.jsonl (one line per judged submission).
"""
import glob, json, os, subprocess, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "oracle"))
import oracle  # noqa: E402

TASKS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "tasks")

def mukoz_verdict(mukoz, task, binary):
    store = binary + ".judge-store"
    out = subprocess.run([mukoz, "check", os.path.join(TASKS_DIR, task, "suite.toml"), "--artifact", binary, "--store", store],
                         capture_output=True, text=True, timeout=600).stdout
    try:
        d = json.loads(out)["data"]
        return d["assessment"]["admission"]
    except Exception:
        return "ERROR"

def main():
    exp, mukoz = sys.argv[1], sys.argv[2]
    dirs = sys.argv[3:] or sorted(glob.glob(os.path.join(exp, "[AB]-*")))
    with open(os.path.join(exp, "results.jsonl"), "a") as f:
        for d in dirs:
            d = d if os.path.isabs(d) else os.path.join(exp, d)
            name = os.path.basename(d.rstrip("/"))
            cond, task = name.split("-", 1)
            sol = os.path.join(d, "solution.bin")
            rec = {"run": name, "condition": cond, "task": task}
            if not os.path.exists(sol):
                rec["oracle"] = "NO_SUBMISSION"
            else:
                j = oracle.judge(task, sol, cases=3000)
                rec.update({"oracle": j["verdict"], "oracle_seed": j["seed"], "oracle_failed": j["failed"],
                            "oracle_first_failures": j["first_failures"][:2],
                            "mukoz_on_final": mukoz_verdict(mukoz, task, sol),
                            "bytes": os.path.getsize(sol)})
            if cond.endswith("A"):
                rec["mukoz_check_runs"] = len(glob.glob(os.path.join(d, ".mukoz", "items", "run-*.json")))
            print(json.dumps(rec))
            f.write(json.dumps(rec) + "\n")

main()
