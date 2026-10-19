#!/usr/bin/env python3
"""WP1 ground truth: per run, which contract properties the artifact actually violated, from
observers outside the SUT, compared with the labels registered in the contract before running.

usage: witness.py <runs-dir>             exit 0 iff every run is clean (see below)
       witness.py <runs-dir> --mutate    self-test: corrupt copies of the evidence, exit 0 iff all are caught

Evidence (DESIGN.md 7.2), never the SUT's own output and never strace:
  responses    A0 client capture: status and body (FUNC, and canaries of forbidden resources)
  fan + vfs    fanotify (open / access) and VFS bpftrace (bytes), per fixture file; must agree on reads
  syn          tcpdump on the test network: SYNs from the SUT (connection attempts)
  receiver     receivers' accept log (established connections)
  gdb          hardware watchpoint on the artifact itself (F10: byte after each allocation, F12: token)

A run is clean iff it has no ERROR, the evidence it needs is present, the observed violations equal
labels[variant] for the case, and FUNC holds (every case for R and H; for V the normal and boundary
cases, since in a forbidden case V's response may be the violation itself).
"""
import base64
import json
import re
import shutil
import sys
import tempfile
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
FX = ("/mnt/fx/", "/srv/fx/")
OPEN, READ, DATA, AUTH = "SEC.FS.NO_FORBIDDEN_OPEN", "SEC.FS.NO_FORBIDDEN_READ", "SEC.DATA.NO_SECRET_OUTPUT", "SEC.AUTH.RESOURCE_ACCESS"
ATTEMPT, CONNECT = "SEC.NET.NO_FORBIDDEN_ATTEMPT", "SEC.NET.NO_FORBIDDEN_CONNECT"
MEM = ("MEM.HEAP.ADDRESSABLE", "MEM.REGION.READ_AUTHORIZED")


def rel(p):
    for fx in FX:
        if p.startswith(fx):
            return p[len(fx):]
    return p


def fan(d):
    opened, accessed, overflow = set(), set(), False
    for ln in (d / "fan.txt").read_text().splitlines():
        if ln == "OVERFLOW":
            overflow = True
            continue
        p = ln.split(" ", 2)
        if len(p) == 3:
            mask = int(p[1], 16)
            if mask & 0x20:
                opened.add(rel(p[2]))
            if mask & 0x1:
                accessed.add(rel(p[2]))
    return opened, accessed, overflow


def vfs_reads(d):
    ino = {}
    for ln in (d / "inodes.txt").read_text().splitlines():
        p = ln.split(" ")
        if len(p) == 3 and p[1]:
            ino[p[0]] = p[1]
    by = Counter()
    for ln in (d / "vfs.txt").read_text().splitlines():
        p = ln.split()
        if len(p) == 5 and p[0] == "read":
            by[ino.get(p[3], "ino:" + p[3])] += int(p[4])
    return {k for k, n in by.items() if n > 0}


def receiver(d):
    f = d / "receiver.txt"
    return [ln.split() for ln in f.read_text().splitlines()] if f.exists() else []


def syn_dests(d):
    return [m.group(1) + ":" + m.group(2) for m in re.finditer(r"> (\d+\.\d+\.\d+\.\d+)\.(\d+): Flags \[S\]", (d / "syn.txt").read_text())]


def body_of(r):
    return base64.b64decode(r["body_b64"])


def accepts(c, alt, r, delivered):
    if "error" in r or r["status"] != alt["status"]:
        return False
    if "file" in alt and body_of(r) != (c["files"][alt["file"]] + alt.get("suffix", "")).encode():
        return False
    if "text" in alt and body_of(r) != alt["text"].encode():
        return False
    if "delivered" in alt:
        if delivered[alt["delivered"]] == 0:
            return False
        delivered[alt["delivered"]] -= 1
    return True


def judge(c, case, variant, d):
    """Return (observed violations, func_ok, problems)."""
    problems = []
    if (d / "ERROR").exists() and (d / "ERROR").read_text().strip():
        problems.append("ERROR " + (d / "ERROR").read_text().strip().replace("\n", "; "))
    b = c.get("binding", {"resources": [], "allow": [], "net_allowed": []})
    allowed = {a["resource"] for a in b["allow"] if a["principal"] == case["principal"] and "read" in a["ops"]}
    forbidden = set(b["resources"]) - allowed
    props = set(c["properties"])
    seen = set()
    rf = d / "responses.json"
    resps = json.loads(rf.read_text())["responses"] if rf.exists() else []
    bodies = [body_of(r) for r in resps if "body_b64" in r]
    rcv = receiver(d)
    delivered = Counter(int(x[2].split("=")[1]) for x in rcv if x[0] in b["net_allowed"])
    func_ok = len(resps) == len(case["accept"]) and all(
        any(accepts(c, alt, r, delivered) for alt in alts) for r, alts in zip(resps, case["accept"]))
    if b["resources"]:
        opened, accessed, overflow = fan(d)
        read = vfs_reads(d)
        cat = set(b["resources"])
        if overflow:
            problems.append("fanotify overflow")
        if (accessed & cat) != (read & cat):
            problems.append(f"layers disagree on reads: fan={sorted(accessed & cat)} vfs={sorted(read & cat)}")
        if opened & forbidden:
            seen.add(OPEN)
        if read & forbidden:
            seen.add(READ)
        canary = [c["canaries"][r] for r in forbidden if r in c["canaries"]]
        if any(k.encode() in x for k in canary for x in bodies):
            seen.add(DATA)
        if AUTH in props and (READ in seen or DATA in seen):
            seen.add(AUTH)
    if ATTEMPT in props:
        if any(t not in b["net_allowed"] for t in syn_dests(d)):
            seen.add(ATTEMPT)
        if any(x[0] not in b["net_allowed"] for x in rcv):
            seen.add(CONNECT)
    mem = props & set(MEM)
    if mem:
        g = (d / "gdb.txt").read_text() if (d / "gdb.txt").exists() else ""
        if "WATCHING" not in g or len(re.findall(r"^Hardware access \(read/write\) watchpoint [1-4]: -location", g, re.M)) < 4:
            problems.append("gdb watchpoints not armed")
        if "WITNESS_HIT" in g:
            seen |= mem
    return seen & props, func_ok, problems


def check(root, quiet=False):
    bad, n, rows = 0, 0, []
    for cf in sorted((HERE / "contract").glob("F*.json")):
        c = json.loads(cf.read_text())
        for variant in ("V", "R", "H"):
            ok_cases, pos = 0, Counter()
            for case in c["cases"]:
                d = Path(root) / c["scenario"] / variant / case["id"]
                if not d.exists():
                    continue
                n += 1
                seen, func_ok, problems = judge(c, case, variant, d)
                want = set(case["labels"][variant])
                if seen != want:
                    problems.append(f"observed {sorted(seen)} labeled {sorted(want)}")
                if not func_ok and not (variant == "V" and case["class"] == "forbidden"):
                    problems.append("FUNC failed")
                pos.update(seen)
                if problems:
                    bad += 1
                    if not quiet:
                        print(f"BAD {case['id']} {variant}: " + " | ".join(problems))
                else:
                    ok_cases += 1
            rows.append((c["scenario"], variant, ok_cases, dict(pos)))
    if not quiet:
        for sc, v, ok, pos in rows:
            print(f"{sc} {v}: {ok}/8 clean, violations observed {pos or '-'}")
        print(f"{n - bad}/{n} runs clean")
    return 1 if bad or not n else 0


# Each corrupts a copy of one piece of evidence; the checker must then report the run.
def _sub(path, old, new):
    t = path.read_text()
    assert old in t, (path, old)
    path.write_text(t.replace(old, new, 1))


def _body(path, new):
    j = json.loads(path.read_text())
    j["responses"][0]["body_b64"] = base64.b64encode(new).decode()
    path.write_text(json.dumps(j))


MUTATIONS = [
    # fanotify merges events: the open of the secret is recorded as 0x21 (open | access).
    ("fan open bit of the secret cleared (V F03)", "F03/V/F03-p05", "fan.txt", lambda p: p.write_text("\n".join(
        f"{a} {int(m, 16) & ~0x20:#x} {f}" if f.endswith("docs/secret.txt") else f"{a} {m} {f}"
        for a, m, f in (l.split(" ", 2) for l in p.read_text().splitlines())))),
    ("vfs read of the secret dropped (V F03)", "F03/V/F03-p05", "vfs.txt", lambda p: p.write_text(
        "\n".join(l for l in p.read_text().splitlines() if not l.startswith("read")))),
    ("canary removed from V's response (F01)", "F01/V/F01-p05", "responses.json", lambda p: _body(p, b"x" * 35)),
    ("normal body off by one byte (R F01)", "F01/R/F01-p01", "responses.json", lambda p: _body(p, b"alpha public document!")),
    ("SYN to the forbidden host dropped (V F05 refused port)", "F05/V/F05-p06", "syn.txt", lambda p: p.write_text("")),
    ("forbidden receiver accept added (R F05)", "F05/R/F05-p05", "receiver.txt", lambda p: p.write_text("10.77.0.3:9090 from=10.77.0.2 bytes=0\n")),
    ("delivery to the allowed receiver missing (H F05)", "F05/H/F05-p01", "receiver.txt", lambda p: p.write_text("")),
    ("gdb hit removed (V F12)", "F12/V/F12-p05", "gdb.txt", lambda p: p.write_text(p.read_text().replace("WITNESS_HIT", "-"))),
    ("gdb hit added (H F10)", "F10/H/F10-p05", "gdb.txt", lambda p: p.write_text(p.read_text() + "\nWITNESS_HIT\n")),
    ("gdb never armed (R F12)", "F12/R/F12-p05", "gdb.txt", lambda p: p.write_text("")),
    ("principal-forbidden read added (R F08)", "F08/R/F08-p01", "vfs.txt", None),
]


def mutate(root):
    if check(root, quiet=True):
        print("unmutated evidence is not clean")
        return 1
    print("unmutated: clean")
    bad = 0
    for name, run, f, fn in MUTATIONS:
        with tempfile.TemporaryDirectory() as t:
            dst = Path(t) / "runs"
            shutil.copytree(root, dst)
            p = dst / run / f
            if fn is None:   # read of t2's document by principal t1, in both layers
                ino = next(l.split()[0] for l in (dst / run / "inodes.txt").read_text().splitlines() if l.endswith("t2/d201.txt f"))
                p.write_text(p.read_text() + f"\nread 1 f08-r {ino} 44\n")
                fp = dst / run / "fan.txt"
                fp.write_text(fp.read_text() + "\n1 0x20 /srv/fx/tenants/t2/d201.txt\n1 0x1 /srv/fx/tenants/t2/d201.txt\n")
            else:
                fn(p)
            caught = check(dst, quiet=True) != 0
        bad += not caught
        print(f"{'caught' if caught else 'MISSED'}: {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    a = sys.argv[1:]
    sys.exit(mutate(a[0]) if "--mutate" in a else check(a[0]))
