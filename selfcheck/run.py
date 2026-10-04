#!/usr/bin/env python3
"""Run the self-check (docs/09 9.6) and write selfcheck/last-run.json.

stage 1  known-answer fixtures and mutants: the cargo test suites (acceptance, coverage, native,
         macho, navigation, platform)
stage 2  the static mukoz CLI as a process under native-process (selfcheck/stage2)
stage 3  the kernels as routines on emulated and native-routine (selfcheck/stage3)

The checker is the mukoz built from this tree, so every result is independence = self unless the
checker's digest is listed in selfcheck/checkers.toml."""
import json, os, subprocess, sys, hashlib, time
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(root)
out = {"started": time.strftime("%Y-%m-%dT%H:%M:%S"), "stages": {}}
subprocess.check_call(["cargo", "build", "--release", "-q"])
mukoz = os.path.join(root, "target/release/mukoz")
out["checker_sha256"] = hashlib.sha256(open(mukoz, "rb").read()).hexdigest()
store = os.path.join(root, "target/selfcheck-store")

t = subprocess.run(["cargo", "test", "--release", "-q"], capture_output=True, text=True)
lines = [l for l in t.stdout.splitlines() if l.startswith("test result")]
out["stages"]["1"] = {"ok": t.returncode == 0, "test_results": lines}

subprocess.check_call(["sh", "selfcheck/stage2/build.sh"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
v = json.loads(subprocess.run([mukoz, "check", "selfcheck/stage2/suite.toml", "--store", store, "--policy", "selfcheck/stage2/policy.toml"], capture_output=True).stdout)
a = v["data"]["assessment"]
observable = [c for c in v["data"]["claims"] if c["property"] not in ("machine.memory.access", "effects.no_forbidden")]
out["stages"]["2"] = {
    "ok": all(c["evaluation"] == "SATISFIED_IN_SCOPE" for c in observable) and all("REQUIRED_CAPABILITY_UNAVAILABLE" in r or r.startswith("SELF_CHECK_ONLY") for r in a["reasons"]),
    "admission": a["admission"], "reasons": a["reasons"], "independence": a["independence"]["value"], "cases": a["scope"]["cases_completed"],
    "subject": a["scope"]["artifact"],
    "note": "native-process observes streams, exit status and files only: memory and effect claims stay NOT_EVALUATED by design",
}
r3 = subprocess.run([sys.executable, "selfcheck/stage3/run.py", mukoz, store, "--json", "target/stage3.json"], capture_output=True, text=True)
s3 = json.load(open("target/stage3.json"))
out["stages"]["3"] = {"ok": r3.returncode == 0, "suites": s3}
json.dump(out, open("selfcheck/last-run.json", "w"), indent=1)
for k, s in out["stages"].items():
    print(f"stage {k}: {'ok' if s['ok'] else 'FAILED'}")
sys.exit(0 if all(s["ok"] for s in out["stages"].values()) else 1)
