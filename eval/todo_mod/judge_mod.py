"""Judge modular todo submissions: flatten the 8 files with the canonical link, run the hidden
oracle natively (../todo/oracle_todo.py, base 0xf0000), and run Mukoz's main suite (with the
routine monitors) on the same files. Also runs each routine's own suite.
usage: python3 judge_mod.py <exp-dir> <mukoz> <run-dir>...   -> appends <exp-dir>/results.jsonl"""
import glob, json, os, shutil, subprocess, sys, tempfile
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "todo"))
import oracle_todo  # noqa: E402
MODS = ["main", "parse_id", "udec", "find_rec", "fmt_line", "make_rec", "del_rec", "clear_done"]

exp, mukoz = sys.argv[1], sys.argv[2]
for d in sys.argv[3:]:
    d = d if os.path.isabs(d) else os.path.join(exp, d)
    name = os.path.basename(d.rstrip("/"))
    rec = {"run": name, "condition": name[1]}
    missing = [m for m in MODS if not os.path.exists(os.path.join(d, m + ".bin"))]
    rec["missing"] = missing
    rec["bytes"] = {m: os.path.getsize(os.path.join(d, m + ".bin")) for m in MODS if m not in missing}
    if missing:
        rec["oracle"] = "INCOMPLETE"
    else:
        with tempfile.TemporaryDirectory() as t:
            subprocess.run([sys.executable, os.path.join(HERE, "make.py"), t], check=True)
            for m in MODS:
                shutil.copy(os.path.join(d, m + ".bin"), os.path.join(t, m + ".bin"))
            img = os.path.join(t, "flat.img")
            subprocess.run([sys.executable, os.path.join(HERE, "flatten.py"), os.path.join(t, "link.toml"), img], check=True)
            j = oracle_todo.judge(img, 300, base="f0000")
            rec.update({"oracle": j["verdict"], "oracle_seed": j["seed"], "oracle_failed_sessions": j["failed_sessions"],
                        "oracle_first_failures": j["first_failures"][:2]})
            mz = {}
            for m in MODS:
                out = subprocess.run([mukoz, "check", os.path.join(t, "check", m, "suite.toml"), "--artifact", os.path.join(t, m + ".bin"), "--store", os.path.join(t, ".st")],
                                     capture_output=True, text=True, timeout=1800).stdout
                try:
                    a = json.loads(out)["data"]["assessment"]
                    mz[m] = [a["admission"]] + a["reasons"][:3]
                except Exception:
                    mz[m] = ["ERROR"]
            rec["mukoz"] = mz
            rec["mukoz_on_final"] = mz["main"][0]
    if name[1] == "A":
        rec["mukoz_check_runs"] = len(glob.glob(os.path.join(d, "**", ".mukoz", "items", "run-*.json"), recursive=True))
    print(json.dumps(rec))
    with open(os.path.join(exp, "results.jsonl"), "a") as f:
        f.write(json.dumps(rec) + "\n")
