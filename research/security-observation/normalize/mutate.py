#!/usr/bin/env python3
"""Calibrate the normalizer's tests: each mutation of strace.py must make test_strace.py or e0_check.py fail.

usage: mutate.py     exit 0 iff the unmutated normalizer passes and every mutation is caught
"""
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = (HERE / "strace.py").read_text()
RUNS = [str(HERE.parent / p) for p in ("e0/runs/r2", "e0/runs/r3", "sbx/runs/s5", "sbx/runs/s6")]

MUTATIONS = [
    ("deleted file resolved", 'if deco is None or deco.endswith(" (deleted)"):', "if deco is None:"),
    ("decoration conflict ignored", "if deco is not None and known is not None and known[0] != deco:", "if False:"),
    ("children do not inherit role", "self.procs[pid] = Proc(pp.role, pp.fds if parent[1] else dict(pp.fds))",
     'self.procs[pid] = Proc("supervisor", pp.fds if parent[1] else dict(pp.fds))'),
    ("unmatched child trusted", "self.procs[pid].uncertain = True", "pass"),
    ("unknown return ignored", 'self.gaps.append({"reason": "unknown_return", "line": lineno})', "pass"),
    ("traced command counted as SUT", 'self.procs[pid] = Proc("supervisor", {})   # the traced command itself',
     'self.procs[pid] = Proc("sut", {})'),
    ("close-on-exec ignored", "for fd in [f for f, (_, cloexec) in p.fds.items() if cloexec]:", "for fd in []:"),
    ("O_PATH counted as read", '"none" if "O_PATH" in flags else ', ""),
    ("EINPROGRESS counted as established", 'self.emit(pid, "net_connect", dest, True, **ref)', 'self.emit(pid, "net_connect", dest, True, **ref)\n            elif err == "EINPROGRESS":\n                self.emit(pid, "net_connect", dest, True, **ref)'),
    ("restart_syscall ignored", 'self.gaps.append({"reason": "restart_syscall", "line": lineno})', "pass"),
    ("strace messages ignored", 'if raw.startswith("strace: "):\n            self.gaps.append({"reason": "strace_message", "line": lineno})\n            return',
     'if raw.startswith("strace: "):\n            return'),
    ("resumed without start accepted", 'if head is None or not head[0].startswith(r.group(1) + "("):', "if head is None:"),
    ("unfinished at end ignored", "if not self.procs.get(pid) or not self.procs[pid].killed:", "if False:"),
    ("forced stop counted as drained", 'elif any(p.killed == "SIGKILL" for p in sut):', "elif False:"),
    ("truncated trace accepted", 'p.gaps.append({"reason": "trace_not_newline_terminated", "line": len(lines)})', "pass"),
    ("pid namespace matching off", "if left and len(left) == len(kids) and", "if False and"),
    ("sendfile source/dest swapped", "out, inn = fd_of(a[0]), fd_of(a[1])", "inn, out = fd_of(a[0]), fd_of(a[1])"),
    ("mmap ignored", 'if f and f[0] != "-1":\n                op', 'if False:\n                op'),
    ("rename destination ignored", "                base = None\n            if ok:", "                break\n            if ok:"),
    ("failed read counted", '{"result": {"success": False, "bytes": -1, "errno": err}}', '{"result": {"success": True, "bytes": 64, "errno": err}}'),
]


def run(src):
    with tempfile.TemporaryDirectory() as d:
        f = Path(d, "strace.py")
        f.write_text(src)
        env = {**os.environ, "NORMALIZER": str(f)}
        t = subprocess.run([sys.executable, str(HERE / "test_strace.py")], env=env, capture_output=True, text=True)
        e = subprocess.run([sys.executable, str(HERE / "e0_check.py"), *RUNS], env=env, capture_output=True, text=True)
        failed = [l for l in t.stderr.splitlines() if l.startswith(("FAIL:", "ERROR:"))]
        bad_runs = [l for l in e.stdout.splitlines() if l.startswith("BAD")]
        return t.returncode, e.returncode, failed, bad_runs


def main():
    t, e, _, _ = run(SRC)
    print(f"unmutated: tests exit {t}, e0_check exit {e}")
    bad = t != 0 or e != 0
    for name, old, new in MUTATIONS:
        n = SRC.count(old)
        if n != 1:
            print(f"NOT APPLIED ({n} matches): {name}")
            bad = True
            continue
        t, e, failed, runs = run(SRC.replace(old, new))
        caught = t != 0 or e != 0
        bad |= not caught
        print(f"{'caught' if caught else 'MISSED'}: {name} (tests failing {len(failed)}, recorded runs BAD {len(runs)})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
