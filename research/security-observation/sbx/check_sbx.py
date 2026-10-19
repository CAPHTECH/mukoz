#!/usr/bin/env python3
"""WP0.7 acceptance checker. usage: check_sbx.py <out-dir>   exit 0 iff every requirement holds."""
import os
import sys
from pathlib import Path

os.environ.setdefault("E0_FX", "/mnt/fx/:/srv/fx/")
os.environ.setdefault("E0_SCOPE_EXEC", "/sut/")
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "e0"))
import check  # noqa: E402  (e0/check.py)

out = Path(sys.argv[1])
bad = 0


def req(name, ok, detail=""):
    global bad
    print(f"{'OK ' if ok else 'NG '} {name}{'  ' + detail if detail and not ok else ''}")
    bad += not ok


def kv(path):
    d = {}
    for ln in path.read_text().splitlines():
        k, _, v = ln.partition(" ")
        d.setdefault(k, v)
        if k in ("readable", "connect"):
            d[f"{k} {v.rsplit(' ', 1)[0]}"] = v.rsplit(" ", 1)[1]
    return d


p = out / "sbx-probe"
k = kv(p / "stdout.txt")
req("probe: runs as obs (uid 1000), no supplementary groups", k.get("uid") == "1000" and k.get("groups") == "1000", k.get("groups", ""))
req("probe: SUT is not pid 1", k.get("pid") not in ("1", None), k.get("pid", ""))
req("probe: no_new_privs, no effective or bounding caps", k.get("NoNewPrivs") == "1" and set(k.get("CapEff", "x")) == {"0"} and set(k.get("CapBnd", "x")) == {"0"})
req("probe: in its case cgroup", "/sec-obs/case-" in k.get("cgroup", ""))
req("probe: cwd is / (not the controller directory)", k.get("cwd", "").startswith("/ "), k.get("cwd", ""))
req("probe: own uts namespace", k.get("hostname", "").startswith("sbx-"), k.get("hostname", ""))
for path in ("/home/obs/e0", "/home/obs/.ssh", "/opt/e0/runs", "/root/.bashrc", "/mnt/fx/secret", "/etc/shadow"):
    req(f"probe: {path} not readable", k.get(f"readable {path}") == "no")
req("probe: sudo fails", k.get("sudo") == "no")
req("probe: only the test-network route", k.get("routes", "").strip(";").count(";") == 0 and k.get("routes", "").startswith("10.77.0.0/24"), k.get("routes", ""))
req("probe: allowed receiver reachable", k.get("connect 10.77.0.1:8081") == "yes")
req("probe: forbidden receiver reachable at OS level (8.3)", k.get("connect 10.77.0.3:9090") == "yes")
for hp in ("192.168.68.76:22", "192.168.68.1:53", "1.1.1.1:443"):
    req(f"probe: {hp} unreachable", k.get(f"connect {hp}") == "no")
# find -xdev still lists the mount points themselves; those are the sandbox's per-case tmpfs.
PER_CASE = {"/home", "/root", "/mnt", "/media", "/tmp", "/var/tmp", "/srv/fx", "/dev/shm", "/run", "/dev/mqueue"}
w = set(k.get("rootfs_writable_dirs", "").split())
req("probe: no writable dir on the root filesystem except per-case tmpfs", w <= PER_CASE, str(sorted(w - PER_CASE)))
o = set(k.get("other_writable_dirs", "").split())
req("probe: no writable dir under /run or /dev except per-case tmpfs", o <= PER_CASE, str(sorted(o - PER_CASE)))
req("probe: fixture writable, /sut read-only", k.get("fixture_writable") == "yes" and k.get("sut_bin_writable") == "no")
rcv = (p / "receiver.txt").read_text() if (p / "receiver.txt").exists() else ""
req("receivers logged both connections", "10.77.0.1:8081 from=10.77.0.2" in rcv and "10.77.0.3:9090 from=10.77.0.2" in rcv, rcv)

i = out / "sbx-inject"
req("inject: exit 3 (BLOCKED)", (i / "exit.txt").read_text().strip() == "3")
req("inject: SUT never started (empty stdout, no main_exit)", (i / "stdout.txt").stat().st_size == 0 and "main_exit" not in (i / "sbx.status").read_text())
req("inject: reason recorded", "BLOCKED check: injected failure" in (i / "sbx.status").read_text())

for it in ("noop", "fdreuse", "fail", "kxfer", "child", "symlink", "restore", "late", "normal"):
    d = out / f"sbx-strace-{it}"
    res, notes = check.check_run(d, check.EXPECT)
    req(f"strace+layers through sandbox: {it}", all(res.values()), "; ".join(notes))

g = out / "sbx-grace0-late"
res, notes = check.check_run(g, {"late": ({}, set(), [])})
req("grace 0: detached child killed before reading (all layers empty)", all(res.values()), "; ".join(notes))
req("grace 1000: status shows main_exit then all_exited", "all_exited" in (out / "sbx-strace-late" / "sbx.status").read_text())

v = out / "sbx-vg-heapover"
kinds = check.parse_mc(v)
req("memcheck in sandbox: InvalidWrite via fd 4", kinds == {"InvalidWrite"}, str(kinds))
req("memcheck in sandbox: SUT stderr untouched", (v / "stderr.txt").stat().st_size == 0)

c = (out / "sbx-pids" / "cgroup.txt").read_text()
pmax = int(c.split("pids.events:max ")[1].split()[0]) if "pids.events:max " in c else 0
req("pids budget: pids.max hit and peak <= 256", pmax > 0 and int(c.split("pids.peak ")[1].split()[0]) <= 256, c)

l = out / "sbx-leftover"
req("leftover: grace expired, cgroup empty and removed", "grace_expired" in (l / "sbx.status").read_text()
    and "leftover_procs 0" in (l / "cgroup.txt").read_text() and "cgroup_not_removed" not in (l / "cgroup.txt").read_text())

print(f"{bad} requirement(s) failed")
sys.exit(1 if bad else 0)
