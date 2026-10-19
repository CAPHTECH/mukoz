#!/usr/bin/env python3
"""Calibrate test_adapter.py: each mutation of adapter.py must make the tests fail.

usage: mutate.py     exit 0 iff the unmutated adapter passes and every mutation is caught
"""
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = (HERE / "adapter.py").read_text()

MUTATIONS = [
    ("logical region decided by memcheck", '"mem.region.read_authorized": ("region",', '"mem.region.read_authorized": ("memcheck",'),
    ("unauthorized region read accepted", "return None if not isinstance(auth, bool) else not auth", "return False"),
    ("zero bytes count as a read", "if not (ok and n > 0):\n            return False\n        return None if res is None else forbidden(binding, principal, res, \"read\")",
     "if not (ok and n >= 0):\n            return False\n        return None if res is None else forbidden(binding, principal, res, \"read\")"),
    ("failed read counts", "if not (ok and n > 0):\n            return False\n        return None if res is None else forbidden(binding, principal, res, \"read\")",
     "if not (n != 0):\n            return False\n        return None if res is None else forbidden(binding, principal, res, \"read\")"),
    ("sendfile not counted as read", '("fs", {"file_read", "file_transfer"})', '("fs", {"file_read"})'),
    ("principal ignored", 'a["principal"] == principal and ', ""),
    ("SUT output trusted", 'e["evidence_origin"] not in TRUSTED_ORIGINS or ', ""),
    ("supervisor counted", ' or e["subject"]["role"] != "sut"', ""),
    ("start not required", 'r["start"] != "before_sut_exec"', "False"),
    ("end not required", 'r["end"] != "drained"', "False"),
    ("gaps ignored", 'if r["gaps"] or r["lost_events"]:', "if False:"),
    ("empty scope accepted", 'case["requests_sent"] == 0', "False"),
    ("stale binding ignored", 'binding_ok = obs["binding_sha256"] == binding_sha', "binding_ok = True"),
    ("stale binding still judges", '        return INC, "BINDING_STALE"\n', "        pass\n"),
    ("unresolved resource treated as allowed", 'return None if res is None else forbidden(binding, principal, res, "read")',
     'return False if res is None else forbidden(binding, principal, res, "read")'),
    ("uncertain attribution ignored", 'if w is None or (w and e["attribution"]["case"] not in CERTAIN_CASE):', "if w is None:"),
    ("violation dropped on log loss", "            return VIO, None\n",
     '            return (INC, "X") if gaps(case, binding_ok) else (VIO, None)\n'),
    ("unsupported event ignored", "unsupported = True", "pass"),
    ("unsupported mechanism ignored", "mechanism |= w is None", "pass"),
    ("unsupported mechanism bears on net", 'if not pred.startswith("fs."):\n            return False', "if False:\n            return False"),
    ("O_PATH open counts as readable", '"rw": ["read", "write"], "none": []}', '"rw": ["read", "write"], "none": ["read"]}'),
    ("missing capability judged", 'if PREDICATES[pred][0] not in case["receipt"]["capabilities"]:', "if False:"),
    ("fill needs every part violated", "if any(ev == VIO for ev, _ in results):", "if all(ev == VIO for ev, _ in results):"),
    ("unknown fields accepted", 'need(not extra, f"{where}: unknown fields {sorted(extra)}")', "pass"),
    ("unfollowed connect counted as not connected", 'if e["stage"] == "attempted":\n            # A non-blocking', 'if False:\n            # A non-blocking'),
    ("leak counted as heap violation", 'return None if kind is None else kind in MEM_INVALID', "return True"),
]


def run(src):
    with tempfile.TemporaryDirectory() as d:
        f = Path(d, "adapter.py")
        f.write_text(src)
        r = subprocess.run([sys.executable, str(HERE / "test_adapter.py")], env={**os.environ, "ADAPTER": str(f)},
                           capture_output=True, text=True)
        tail = [l for l in r.stderr.splitlines() if l.startswith(("FAIL:", "ERROR:"))]
        return r.returncode, tail


def main():
    code, _ = run(SRC)
    print(f"unmutated: exit {code}")
    bad = code != 0
    for name, old, new in MUTATIONS:
        n = SRC.count(old)
        if n != 1:
            print(f"NOT APPLIED ({n} matches): {name}")
            bad = True
            continue
        code, failed = run(SRC.replace(old, new))
        caught = code != 0
        bad |= not caught
        print(f"{'caught' if caught else 'MISSED'}: {name} ({len(failed)} failing, e.g. {failed[:1]})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
