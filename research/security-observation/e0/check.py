#!/usr/bin/env python3
"""E0 checker: compare each observation layer of a run against the calibration expectation.

usage: check.py <runs-dir>            exit 0 iff every calibration run matches on every layer
       check.py <runs-dir> --mutate   self-test: corrupt expectations, exit 0 iff all are caught

Layers: strace (B), vfs (bpftrace on VFS functions, byte counts), fan (fanotify, presence),
mc (Memcheck error kinds). Only files on the fixture filesystem (/mnt/fx) are counted.
"""
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

# Fixture path prefixes. In the sandbox the SUT sees /srv/fx while fanotify reports /mnt/fx.
FX = os.environ.get("E0_FX", "/mnt/fx/").split(":")

# item -> bytes per fixture file {name: (read, write)}, files opened, failed attempts
EXPECT = {
    "noop": ({}, set(), []),
    "fdreuse": ({"secret": (10, 0), "decoy": (25, 0)}, {"secret", "decoy"}, []),
    "fail": ({}, {"out/w"}, [("openat", "missing", "ENOENT"), ("openat", "secret", "EACCES"),
                             ("read", "out/w", "EBADF"), ("connect", "127.0.0.1:1", "ECONNREFUSED")]),
    "kxfer": ({"secret": (12288, 0), "out/sf": (0, 4096), "out/cfr": (0, 4096), "out/sp": (0, 4096)},
              {"secret", "out/sf", "out/cfr", "out/sp"}, []),
    "child": ({"secret": (4096, 0), "decoy": (4096, 0), "out/child1": (0, 4096), "out/child2": (0, 4096)},
              {"secret", "decoy", "out/child1", "out/child2"}, []),
    "symlink": ({"secret": (100, 0)}, {"secret"}, []),
    "restore": ({"rw": (8192, 8192)}, {"rw"}, []),
    "late": ({"secret": (64, 0)}, {"secret"}, []),
    "normal": ({"public": (4096, 0), "out/ok": (0, 3)}, {"public", "out/ok"}, []),
    "heapover": ({}, set(), []),
    "uaf": ({}, set(), []),
    "logical": ({}, set(), []),
}
# Memcheck kinds expected at -O0. At -O2 the heap items lost their errors in run 1.
MC_EXPECT = {"heapover": {"InvalidWrite"}, "uaf": {"InvalidRead"}}
# Runs reported but not required to match: the expectation itself is what is being learned.
EXPLORE = re.compile(r"^(ns-strace-late|vg-memcheck-cal-(heapover|uaf)|atime-.*)$")

LINE = re.compile(r"^(\d+)\s+(.*)$")
CALL = re.compile(r"^(\w+)\((.*)\)\s+=\s+(-?\d+|\?)(?:\s+(\w+))?")
FDP = re.compile(r"(\d+)<([^>]*)>")


def rel(p):
    for fx in FX:
        if p.startswith(fx):
            return p[len(fx):]
    return None


def parse_strace(path):
    """Return ({name: [read, write]}, opened set, failed attempts) from strace -f -yy output."""
    by = defaultdict(lambda: [0, 0])
    opened, fails, pend = set(), [], {}
    # Scope receipt: with E0_SCOPE_EXEC set (e.g. "/sut/"), events before the first execve of a
    # path under it belong to the supervisor and are not counted.
    scope = os.environ.get("E0_SCOPE_EXEC")
    started = scope is None
    for raw in path.read_text(errors="replace").splitlines():
        m = LINE.match(raw)
        if not m:
            continue
        pid, rest = m.groups()
        if rest.endswith("<unfinished ...>"):
            pend[pid] = rest[: -len("<unfinished ...>")]
            continue
        r = re.match(r"^<\.\.\. \w+ resumed>(.*)$", rest)
        if r:
            rest = pend.pop(pid, "") + r.group(1)
        if not started:
            # The SUT's execve may be split into "unfinished" and "resumed" lines (acceptance run s4).
            started = rest.startswith(f'execve("{scope}') and bool(re.search(r"\)\s+=\s+0$", rest))
            continue
        c = CALL.match(rest)
        if not c:
            continue
        name, args, ret, err = c.groups()
        if ret == "?":
            continue
        ret = int(ret)
        fds = FDP.findall(args)
        if name in ("openat", "open"):
            q = re.search(r'"([^"]*)"', args)
            if ret < 0 and q:
                if rel(q.group(1)):   # failures outside the fixture (locale, valgrind files) are not effects
                    fails.append((name, rel(q.group(1)), err))
            else:
                t = FDP.search(rest.split("=")[-1])
                if t and rel(t.group(2)):
                    opened.add(rel(t.group(2)))
            continue
        if name == "connect" and ret < 0:
            a = re.search(r'sin_port=htons\((\d+)\).*inet_addr\("([^"]+)"\)', args)
            fails.append(("connect", f"{a.group(2)}:{a.group(1)}" if a else "?", err))
            continue
        if ret < 0:
            if fds and rel(fds[0][1]) and name in ("read", "pread64", "readv", "write", "pwrite64", "writev"):
                fails.append((name, rel(fds[0][1]), err))
            continue
        if name in ("read", "pread64", "readv", "preadv", "preadv2") and fds and rel(fds[0][1]):
            by[rel(fds[0][1])][0] += ret
        elif name in ("write", "pwrite64", "writev", "pwritev", "pwritev2") and fds and rel(fds[0][1]):
            by[rel(fds[0][1])][1] += ret
        elif name in ("sendfile", "sendfile64") and len(fds) >= 2:
            out, inn = fds[0][1], fds[1][1]
            if rel(inn): by[rel(inn)][0] += ret
            if rel(out): by[rel(out)][1] += ret
        elif name in ("copy_file_range", "splice") and len(fds) >= 2:
            inn, out = fds[0][1], fds[1][1]
            if rel(inn): by[rel(inn)][0] += ret
            if rel(out): by[rel(out)][1] += ret
    return {k: tuple(v) for k, v in by.items() if v != [0, 0]}, opened, fails


def inode_map(d):
    m = {}
    for ln in (d / "inodes.txt").read_text().splitlines():
        parts = ln.split(" ")
        if len(parts) == 3 and parts[1]:
            m[parts[0]] = parts[1]
    return m


def parse_vfs(d):
    ino = inode_map(d)
    by = defaultdict(lambda: [0, 0])
    for ln in (d / "vfs.txt").read_text().splitlines():
        p = ln.split()
        if len(p) != 5 or p[0] not in ("read", "write"):
            continue
        name = ino.get(p[3], "ino:" + p[3])
        by[name][0 if p[0] == "read" else 1] += int(p[4])
    return {k: tuple(v) for k, v in by.items()}


def parse_fan(d):
    acc, mod, opn, overflow = set(), set(), set(), False
    for ln in (d / "fan.txt").read_text().splitlines():
        if ln == "OVERFLOW":
            overflow = True
            continue
        p = ln.split(" ", 2)
        if len(p) != 3:
            continue
        name, mask = rel(p[2]) or p[2], int(p[1], 16)
        if mask & 0x1: acc.add(name)
        if mask & 0x2: mod.add(name)
        if mask & 0x20: opn.add(name)
    return acc, mod, opn, overflow


def parse_mc(d):
    x = d / "vg.xml"
    if x.exists():
        return set(re.findall(r"<kind>(\w+)</kind>", x.read_text()))
    log = d / "vg.log"
    if not log.exists():
        return None
    t = log.read_text()
    kinds = set()
    if "Invalid write" in t: kinds.add("InvalidWrite")
    if "Invalid read" in t: kinds.add("InvalidRead")
    return kinds if "Memcheck" in t else None


def item_of(run):
    return run.split("-")[-1]


def check_run(d, expect):
    item = item_of(d.name)
    if item not in expect:
        return None
    eb, eopen, efail = expect[item]
    res, notes = {}, []
    st = d / "strace.txt"
    if st.exists():
        sb, sopen, sfail = parse_strace(st)
        ok = sb == eb and sopen == eopen and all(f in sfail for f in efail) and len(sfail) == len(efail)
        res["strace"] = ok
        if not ok: notes.append(f"strace bytes={sb} opened={sorted(sopen)} fails={sfail}")
    vb = parse_vfs(d)
    res["vfs"] = vb == eb
    if vb != eb: notes.append(f"vfs bytes={vb}")
    acc, mod, opn, ovf = parse_fan(d)
    erd = {k for k, (r, w) in eb.items() if r}
    ewr = {k for k, (r, w) in eb.items() if w}
    ok = acc == erd and mod == ewr and opn == eopen and not ovf
    res["fan"] = ok
    if not ok: notes.append(f"fan access={sorted(acc)} modify={sorted(mod)} open={sorted(opn)} overflow={ovf}")
    if "vg-none" in d.name or "vg-memcheck" in d.name or "strace-vg" in d.name or "ns-vg" in d.name:
        mc = parse_mc(d)
        if mc is not None:
            want = MC_EXPECT.get(item, set())
            res["mc"] = mc == want
            if mc != want: notes.append(f"memcheck kinds={sorted(mc)} expected={sorted(want)}")
    if item == "restore":
        same = (d / "rw.before.sha256").read_text() == (d / "rw.sha256").read_text()
        notes.append(f"rw hash unchanged={same}")
    return res, notes


def mutated():
    m = {k: (dict(v[0]), set(v[1]), list(v[2])) for k, v in EXPECT.items()}
    m["fdreuse"] = ({"secret": (25, 0), "decoy": (10, 0)}, {"secret", "decoy"}, [])     # FD misattribution
    m["kxfer"] = ({"secret": (8192, 0), "out/sf": (0, 4096), "out/cfr": (0, 4096)}, m["kxfer"][1], [])  # lost splice
    m["symlink"] = ({"link": (100, 0)}, {"link"}, [])                                     # display path
    m["restore"] = ({"rw": (8192, 0)}, {"rw"}, [])                                        # missed write
    m["late"] = ({}, set(), [])                                                           # missed late read
    return m


def main():
    runs = Path(sys.argv[1])
    mutate = "--mutate" in sys.argv
    expect = mutated() if mutate else EXPECT
    bad = caught = 0
    for d in sorted(p for p in runs.iterdir() if p.is_dir()):
        r = check_run(d, expect)
        if r is None:
            continue
        res, notes = r
        explore = bool(EXPLORE.match(d.name))
        status = "OK" if all(res.values()) else ("EXPLORE" if explore else "FAIL")
        if mutate:
            if item_of(d.name) in ("fdreuse", "kxfer", "symlink", "restore", "late") and not explore and "vg" not in d.name:
                if status == "OK":
                    print(f"NOT CAUGHT {d.name}")
                    bad += 1
                else:
                    caught += 1
            continue
        if status == "FAIL":
            bad += 1
        layers = " ".join(f"{k}={'ok' if v else 'NG'}" for k, v in res.items())
        print(f"{status:8} {d.name:34} {layers}")
        for n in notes:
            print(f"         {n}")
    if mutate:
        print(f"mutations caught in {caught} runs, missed in {bad}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
