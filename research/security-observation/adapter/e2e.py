#!/usr/bin/env python3
"""End-to-end: artificial observations -> adapter.py -> mukoz check --external-claims --gate.

usage: e2e.py <mukoz-repo> [<mukoz-binary>]   exit 0 iff every scenario gets the expected admission

Uses the native-process hello suite (4 cases rnd-00000..3), whose effects.no_forbidden and
machine.memory.access are NOT_EVALUATED / REQUIRED_CAPABILITY_UNAVAILABLE without the flag.
Each mukoz run gets a fresh store, so no regression shifts the case IDs (docs/07 limitation).
"""
import copy
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adapter as A  # noqa: E402
import test_adapter as T  # noqa: E402

SUITE = "examples/hello/suite.x86_64.elf.native.toml"
ART = "fixtures/process/hello_x86.elf"
ISOLATION = '["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "credential_isolation"]'
CASES = [f"rnd-{i:05d}" for i in range(4)]

BINDING = copy.deepcopy(T.BINDING)
# [R] Filling machine.memory.access from Memcheck heap addressability is weaker than the Mukoz property
# (allowed_read regions, docs/06 6.2). It is done here only to reach ACCEPT through the flag.
BINDING["fills"]["machine.memory.access"] = [T.HEAP]
BRAW = json.dumps(BINDING).encode()


def mukoz(repo, binary, store, extra):
    args = [binary, "check", SUITE, "--artifact", ART, "--store", str(store), "--policy", str(store / "policy.toml")] + extra
    r = subprocess.run(args, cwd=repo, capture_output=True, text=True)
    return r.returncode, json.loads(r.stdout)


def fresh(tmp, name, repo):
    d = Path(tmp, name)
    d.mkdir()
    (d / "policy.toml").write_text(
        f'[[native_trial_zones]]\nid = "p"\nartifact_dir = "{Path(repo, "fixtures")}"\nexecutors = ["native-process"]\n'
        f'targets = ["x86_64/elf/sysv-x86_64/linux"]\nrequire_isolation = {ISOLATION}\nmax_wall_ms_per_case = 2000\n')
    return d


def obs(cases):
    return {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(BRAW).hexdigest(),
            "collectors": [{"name": "artificial", "version": "0"}], "cases": cases}


def scenarios():
    clean = [T.clean_case(c) for c in CASES]

    def edit(f):
        cs = copy.deepcopy(clean)
        f(cs)
        return cs

    def violate(cs):
        cs[2]["events"].append(T.ev("file_read", "doc-secret", n=32))

    def start_missing(cs):
        cs[1]["receipt"]["start"] = None

    def fake_pass(cs):
        cs[1]["receipt"]["start"] = None
        e = T.ev("file_read", "doc-public", n=1, detail={"text": "PASS " + T.SutOutput.FAKE})
        e["evidence_origin"] = "sut_output"
        cs[1]["events"].append(e)

    def violate_after_loss(cs):
        violate(cs)
        cs[2]["receipt"]["lost_events"] = 5
        cs[3]["receipt"]["end"] = None

    def vacuous(cs):
        for c in cs:
            c["requests_sent"] = 0

    # name, cases, independence, expected (exit, admission or error code)
    return [
        ("clean", clean, "independent", (0, "ACCEPT_WITHIN_SCOPE")),
        ("forbidden-read", edit(violate), "independent", (11, "REJECT")),
        ("read-failed", edit(lambda cs: cs[2]["events"].append(T.ev("file_read", "doc-secret", ok=False, n=-1))), "independent", (0, "ACCEPT_WITHIN_SCOPE")),
        ("sendfile-positive", edit(lambda cs: cs[0]["events"].append(T.ev("file_transfer", "doc-secret", n=4096, dest_resource_id="sock", mechanism="sendfile"))), "independent", (11, "REJECT")),
        ("sendfile-zero", edit(lambda cs: cs[0]["events"].append(T.ev("file_transfer", "doc-secret", n=0, dest_resource_id="sock", mechanism="sendfile"))), "independent", (0, "ACCEPT_WITHIN_SCOPE")),
        ("principal-bob", edit(lambda cs: [cs[2].update(principal="bob"), cs[2]["events"].append(T.ev("file_read", "doc-secret", n=32))]), "independent", (0, "ACCEPT_WITHIN_SCOPE")),
        ("principal-alice", edit(lambda cs: cs[2]["events"].append(T.ev("file_read", "doc-secret", n=32))), "independent", (11, "REJECT")),
        ("start-missing", edit(start_missing), "independent", (10, "HOLD")),
        ("violation-then-loss", edit(violate_after_loss), "independent", (11, "REJECT")),
        ("sut-fake-pass", edit(fake_pass), "independent", (10, "HOLD")),
        ("case-missing", clean[:3], "independent", (10, "HOLD")),
        ("no-cases", [], "independent", (10, "HOLD")),
        ("vacuous", edit(vacuous), "independent", (10, "HOLD")),
        ("independence-undeclared", clean, "undeclared", (10, "HOLD")),
    ]


def main(argv):
    repo = Path(argv[0]).resolve()
    binary = str(Path(argv[1]).resolve()) if len(argv) > 1 else str(repo / "target/release/mukoz")
    bad = 0
    with tempfile.TemporaryDirectory() as tmp:
        code, control = mukoz(repo, binary, fresh(tmp, "control", repo), ["--gate"])
        ctx = control["data"]["assessment"]["subject_context"]
        print(f"control (no flag): exit {code} {control['data']['assessment']['admission']}")
        runs = scenarios() + [("stale-context", [T.clean_case(c) for c in CASES], "independent", (2, "STALE_CONTEXT"))]
        for name, cases, indep, (want_code, want) in runs:
            o = obs(cases)
            use = ctx if name != "stale-context" else ("0" if ctx[0] != "0" else "1") + ctx[1:]
            out, _ = A.convert(BRAW, BINDING, json.dumps(o).encode(), o, use, indep)
            st = fresh(tmp, name, repo)
            f = st / "claims.json"
            f.write_text(json.dumps(out))
            code, v = mukoz(repo, binary, st, ["--gate", "--external-claims", str(f)])
            got = v["data"]["assessment"]["admission"] if v["ok"] else v["errors"][0]["code"]
            why = (v["data"]["assessment"].get("reasons") if v["ok"] else None) or ""
            ok = (code, got) == (want_code, want)
            bad += not ok
            print(f"{'ok ' if ok else 'BAD'} {name}: exit {code} {got} {why} (claims {len(out['claims'])})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
