#!/usr/bin/env python3
"""Normalize strace -f -yy output into secobs.observations/0 (WP3, DESIGN.md 9.2-9.7).

usage: strace.py --binding B --case ID=TRACE [--case ...] --principal P --requests-sent N
                 --fx PREFIX [--fx ...] [--sut-exec PREFIX] [--controller-end END] -o OUT

Expected capture: strace -f -yy [-qq] [-s N] -e trace=%file,%desc,%network,%process -o TRACE <cmd>
Exit 0 on success, 2 on unusable input (nothing is written).

Scope: events before the SUT's execve belong to the supervisor. The SUT is the first process whose
successful execve path starts with --sut-exec (default: the first successful execve in the trace),
together with every process it creates afterwards.
Resource identity: the -yy decoration of each fd, cross-checked against the FD lineage this parser
keeps (open/dup/fcntl/close/clone/exec with O_CLOEXEC). Paths under a --fx prefix become catalog IDs
relative to it; others stay raw paths. Missing or conflicting identity leaves the resource unresolved.
"""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

CAPABILITIES = ["fs", "net", "proc"]
READS = {"read", "pread64", "readv", "preadv", "preadv2"}
WRITES = {"write", "pwrite64", "writev", "pwritev", "pwritev2"}
OPENS = {"open", "openat", "openat2", "creat"}
EXECS = {"execve", "execveat"}
FORKS = {"clone", "clone3", "fork", "vfork"}
# Mechanisms the first B does not follow (9.2). Seeing one is recorded, never silently allowed.
UNSUPPORTED = {"io_uring_setup", "io_uring_enter", "io_uring_register", "vmsplice", "process_vm_readv",
               "process_vm_writev", "pidfd_getfd", "tee"}
MUTATIONS = {"rename", "renameat", "renameat2", "unlink", "unlinkat", "truncate", "ftruncate", "link",
             "linkat", "symlink", "symlinkat", "chmod", "fchmod", "fchmodat", "chown", "fchown", "fchownat",
             "lchown", "fallocate", "utimensat", "mknod", "mknodat"}

LINE = re.compile(r"^(\d+)\s+(.*)$")
DONE = re.compile(r"^(?P<name>\w+)\((?P<args>.*)\)\s+=\s+(?P<ret>-?\d+|0x[0-9a-f]+|\?)(?P<deco><.*>)?"
                  r"(?:\s+/\* (?P<nspid>\d+) in strace's PID NS \*/)?(?:\s+(?P<err>[A-Z][A-Z0-9_]+))?(?:\s+\(.*\))?$")
EXIT = re.compile(r"^\+\+\+ (exited with \d+|killed by (SIG\w+)(?: \(core dumped\))?) \+\+\+$")
SIGNAL = re.compile(r"^--- SIG\w+ .*---$")
RESUMED = re.compile(r"^<\.\.\. (\w+) resumed>(.*)$")
FDARG = re.compile(r"^(-?\d+|AT_FDCWD)<(.*)>$", re.S)


class Unusable(Exception):
    pass


def split_args(s):
    """Split a strace argument list at top-level commas (quotes, brackets and fd decorations nest)."""
    out, depth, cur, i, q = [], 0, [], 0, False
    while i < len(s):
        c = s[i]
        if q:
            cur.append(c)
            if c == "\\" and i + 1 < len(s):
                cur.append(s[i + 1])
                i += 1
            elif c == '"':
                q = False
        elif c == '"':
            q = True
            cur.append(c)
        elif c in "([{" or (c == "<" and cur and (cur[-1].isdigit() or "".join(cur).endswith("AT_FDCWD") or depth)):
            depth += 1
            cur.append(c)
        elif c in ")]}>" and depth:
            depth -= 1
            cur.append(c)
        elif c == "," and depth == 0:
            out.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    if cur or out:
        out.append("".join(cur).strip())
    return out


def unquote(a):
    m = re.match(r'^"((?:[^"\\]|\\.)*)"(\.\.\.)?$', a)
    if not m or m.group(2):
        return None   # absent or truncated by -s: not a path we can use
    return re.sub(r"\\(.)", r"\1", m.group(1))


def fd_of(a):
    """(fd, decoration or None) for an fd argument, None if the argument is not an fd."""
    m = FDARG.match(a)
    if m:
        return m.group(1), m.group(2)
    if re.fullmatch(r"-?\d+", a):
        return a, None
    return None


class Proc:
    def __init__(self, role, fds):
        self.role, self.fds, self.exited, self.killed = role, fds, False, None
        self.uncertain = False


class Parser:
    def __init__(self, fx, sut_exec):
        self.fx, self.sut_exec = fx, sut_exec
        self.procs, self.pending, self.events, self.gaps = {}, {}, [], []
        self.started = False
        self.lost = 0
        self.parents = {}   # child pid -> (parent pid, shares fd table), from pass 1

    # identity -----------------------------------------------------------------
    def resource(self, deco):
        if deco is None or deco.endswith(" (deleted)"):
            return None
        for p in self.fx:
            if deco.startswith(p):
                return deco[len(p):]
        return deco

    def resolve(self, proc, fd, deco):
        """Resource of fd from the decoration and the lineage; None if missing or conflicting."""
        known = proc.fds.get(fd)
        if deco is not None and known is not None and known[0] != deco:
            return None
        d = deco if deco is not None else (known[0] if known else None)
        return self.resource(d)

    def proc(self, pid):
        if pid not in self.procs:
            parent = self.parents.get(pid)
            if parent and parent[0] in self.procs:
                # With -f the child's first lines can precede the parent's clone return.
                pp = self.procs[parent[0]]
                self.procs[pid] = Proc(pp.role, pp.fds if parent[1] else dict(pp.fds))
                self.procs[pid].uncertain = pp.uncertain
            elif not self.procs:
                self.procs[pid] = Proc("supervisor", {})   # the traced command itself
            else:
                # Creation not matched: after the SUT start it may be the SUT's, so its events can block
                # SATISFIED but cannot be a counterexample.
                self.procs[pid] = Proc("sut" if self.started else "supervisor", {})
                self.procs[pid].uncertain = True
        return self.procs[pid]

    # events -------------------------------------------------------------------
    def emit(self, pid, kind, res, ok, n=None, **kw):
        p = self.proc(pid)
        e = {"subject": {"role": p.role, "pid": int(pid)}, "event_kind": kind,
             "stage": "completed" if ok else "failed", "resource_id": res, "result": {"success": ok},
             "attribution": {"case": "pid_unmatched" if p.uncertain else "exclusive_process_group"},
             "evidence_origin": "os_observer"}
        if n is not None:
            e["result"]["bytes"] = n
        e.update(kw)
        self.events.append(e)

    def call(self, pid, name, args, ret, retdeco, err, lineno):
        p = self.proc(pid)
        a = split_args(args)
        ok = ret not in ("?",) and not ret.startswith("-")
        rv = int(ret, 0) if ret != "?" else None
        ref = {"raw_evidence_ref": f"line/{lineno}"}
        if ret == "?" and name not in ("exit", "exit_group") and name not in EXECS:
            if name in READS | WRITES | {"sendfile", "sendfile64", "copy_file_range", "splice"} | OPENS:
                self.gaps.append({"reason": "unknown_return", "line": lineno})
            return
        if name in EXECS:
            path = unquote(a[1] if name == "execveat" else a[0]) if a else None
            if rv == 0:
                if not self.started and (self.sut_exec is None or (path or "").startswith(self.sut_exec)):
                    self.started = True
                    p.role = "sut"
                # Close-on-exec descriptors do not survive a successful exec.
                for fd in [f for f, (_, cloexec) in p.fds.items() if cloexec]:
                    del p.fds[fd]
            self.emit(pid, "process_exec", path, rv == 0, **ref)
            return
        if name in FORKS:
            if ok and rv and str(rv) not in self.procs and str(rv) in self.parents:
                shared = "CLONE_FILES" in args or "CLONE_THREAD" in args
                self.procs[str(rv)] = Proc(p.role, p.fds if shared else dict(p.fds))
            return
        if name in ("exit", "exit_group"):
            return
        if name in OPENS:
            if name == "openat2":
                path, flags = unquote(a[1]), a[2]
            elif name == "openat":
                path, flags = unquote(a[1]), a[2]
            elif name == "open":
                path, flags = unquote(a[0]), a[1]
            else:
                path, flags = unquote(a[0]), "O_WRONLY|O_CREAT|O_TRUNC"
            acc = "none" if "O_PATH" in flags else "rw" if "O_RDWR" in flags else "write" if "O_WRONLY" in flags else "read"
            if ok:
                deco = retdeco[1:-1] if retdeco else None
                if deco is None:
                    p.fds.pop(ret, None)
                else:
                    p.fds[ret] = (deco, "O_CLOEXEC" in flags)
                self.emit(pid, "file_open", self.resource(deco), True, access=acc, **ref)
            else:
                # A failed open has no fd: the requested path is all there is, unresolved through symlinks.
                if path and not path.startswith("/"):
                    base = fd_of(a[0]) if name in ("openat", "openat2") else None
                    path = base[1] + "/" + path if base and base[1] else None
                if path and "/../" in path + "/":
                    path = None
                self.emit(pid, "file_open", self.resource(path) if path else None, False, access=acc,
                          result={"success": False, "errno": err}, **ref)
            return
        if name in ("dup", "dup2", "dup3") or (name == "fcntl" and len(a) > 1 and a[1].startswith("F_DUPFD")):
            src = fd_of(a[0]) if a else None
            if ok and src:
                deco = src[1] if src[1] is not None else (p.fds.get(src[0]) or (None,))[0]
                cloexec = (name == "dup3" and "O_CLOEXEC" in args) or (name == "fcntl" and "CLOEXEC" in a[1])
                if deco is None:
                    p.fds.pop(str(rv), None)
                else:
                    p.fds[str(rv)] = (deco, cloexec)
            return
        if name == "fcntl" and len(a) > 2 and a[1] == "F_SETFD":
            f = fd_of(a[0])
            if ok and f and f[0] in p.fds:
                p.fds[f[0]] = (p.fds[f[0]][0], "FD_CLOEXEC" in a[2])
            return
        if name == "close":
            f = fd_of(a[0]) if a else None
            if f:
                p.fds.pop(f[0], None)
            return
        if name in READS or name in WRITES:
            f = fd_of(a[0]) if a else None
            if f is None:
                self.gaps.append({"reason": "fd_unparsed", "line": lineno})
                return
            kind = "file_read" if name in READS else "file_write"
            extra = {} if ok else {"result": {"success": False, "bytes": -1, "errno": err}}
            self.emit(pid, kind, self.resolve(p, *f), ok, rv if ok else -1, mechanism=name, **extra, **ref)
            return
        if name in ("sendfile", "sendfile64", "copy_file_range", "splice"):
            if name.startswith("sendfile"):
                out, inn = fd_of(a[0]), fd_of(a[1])
            else:
                inn, out = fd_of(a[0]), fd_of(a[2])
            if inn is None or out is None:
                self.gaps.append({"reason": "fd_unparsed", "line": lineno})
                return
            self.emit(pid, "file_transfer", self.resolve(p, *inn), ok, rv if ok else -1,
                      dest_resource_id=self.resolve(p, *out), mechanism=name, **ref)
            return
        if name == "connect":
            dest = None
            m = re.search(r'sin_port=htons\((\d+)\).*?inet_addr\("([^"]+)"\)', args)
            m6 = re.search(r'sin6_port=htons\((\d+)\).*?inet_pton\(AF_INET6, "([^"]+)"', args)
            mu = re.search(r'sun_path=("(?:[^"\\]|\\.)*")', args)
            if m:
                dest = f"{m.group(2)}:{m.group(1)}"
            elif m6:
                dest = f"[{m6.group(2)}]:{m6.group(1)}"
            elif mu:
                dest = "unix:" + (unquote(mu.group(1)) or "")
            if ok:
                self.emit(pid, "net_connect", dest, True, **ref)
            else:
                # EINPROGRESS is an attempt whose outcome this blocking-I/O profile does not follow (9.2).
                e = {"success": False, "errno": err}
                self.emit(pid, "net_connect", dest, False, result=e, **ref)
                if err == "EINPROGRESS":
                    self.events[-1]["stage"] = "attempted"
            return
        if name in ("mmap", "mmap2") and len(a) >= 5:
            f = fd_of(a[4])
            if f and f[0] != "-1":
                op = "write" if "PROT_WRITE" in a[2] and "MAP_SHARED" in a[3] else "read"
                self.emit(pid, "unsupported_mechanism", self.resolve(p, *f), ok, mechanism=name, op=op, **ref)
            return
        if name in ("sendmsg", "recvmsg") and "SCM_RIGHTS" in args:
            self.emit(pid, "unsupported_mechanism", None, ok, mechanism="fd_passing", **ref)
            return
        if name in UNSUPPORTED:
            self.emit(pid, "unsupported_mechanism", None, ok, mechanism=name, **ref)
            return
        if name in MUTATIONS:
            # Every path or fd argument may be a target (rename replaces its destination, link adds a name).
            targets, base = [], None
            for x in a:
                f = fd_of(x)
                if f:
                    if name.startswith("f") and f[0] != "AT_FDCWD" and not targets:
                        targets.append(self.resolve(p, *f))
                    base = f[1]
                    continue
                q = unquote(x)
                if q is None:
                    continue
                full = q if q.startswith("/") else (base + "/" + q if base else None)
                targets.append(self.resource(full) if full and "/../" not in full + "/" else None)
                base = None
            if ok:
                for t in targets or [None]:
                    self.emit(pid, "unsupported_mechanism", t, True, mechanism=name, op="write", **ref)
            return
        if name == "restart_syscall":
            self.gaps.append({"reason": "restart_syscall", "line": lineno})

    def line(self, raw, lineno):
        if raw.startswith("strace: "):
            self.gaps.append({"reason": "strace_message", "line": lineno})
            return
        m = LINE.match(raw)
        if not m:
            self.lost += 1
            self.gaps.append({"reason": "unparsed", "line": lineno})
            return
        pid, rest = m.groups()
        if rest.endswith(" <unfinished ...>"):
            if pid in self.pending:
                self.gaps.append({"reason": "unfinished_overwritten", "line": lineno})
            self.pending[pid] = (rest[: -len(" <unfinished ...>")], lineno)
            return
        r = RESUMED.match(rest)
        if r:
            head = self.pending.pop(pid, None)
            if head is None or not head[0].startswith(r.group(1) + "("):
                self.lost += 1
                self.gaps.append({"reason": "resumed_without_start", "line": lineno})
                return
            rest = head[0] + r.group(2)
        x = EXIT.match(rest)
        if x:
            p = self.proc(pid)
            p.exited, p.killed = True, x.group(2)
            return
        if SIGNAL.match(rest):
            return
        c = DONE.match(rest)
        if not c:
            self.lost += 1
            self.gaps.append({"reason": "unparsed", "line": lineno})
            return
        name, args, ret, retdeco, err = c.group("name", "args", "ret", "deco", "err")
        self.proc(pid)
        if name in ("exit", "exit_group"):
            self.procs[pid].exited = True
        self.call(pid, name, args, ret, retdeco, err, lineno)

    def receipt(self, controller_end):
        for pid, (_, lineno) in self.pending.items():
            # A process killed inside a syscall leaves it unfinished; that is covered by its kill record.
            if not self.procs.get(pid) or not self.procs[pid].killed:
                self.gaps.append({"reason": "unfinished_without_resume", "line": lineno})
        sut = [p for p in self.procs.values() if p.role == "sut"]
        if controller_end not in (None, "drained"):
            end = controller_end
        elif not self.started or any(not p.exited for p in sut):
            end = "process_exit_unobserved"
        elif any(p.killed == "SIGKILL" for p in sut):
            end = "forced_stop"
        else:
            end = "drained"
        return {"start": "before_sut_exec" if self.started else None, "end": end, "gaps": self.gaps,
                "lost_events": self.lost, "capabilities": list(CAPABILITIES)}


def fork_parents(lines):
    """Pass 1: child pid -> (parent pid, shares fd table, matched exactly).

    clone() returns the child's pid in the caller's pid namespace, while strace prints pids of its own
    namespace (E0, 8.2). A child is matched exactly by the return value or by --pidns-translation's
    comment. The rest are matched by allocation order: pids grow in both namespaces, so the unmatched
    calls sorted by returned pid pair with the unmatched children sorted by pid, provided the counts are
    equal and every call starts before its child's first line [R: breaks with pid wraparound or several
    nested namespaces]. Otherwise a child is matched only to the one unconsumed call started before it.
    Children left unmatched are treated as of uncertain role.
    """
    calls, pend, first = [], {}, {}
    for i, raw in enumerate(lines):
        m = LINE.match(raw)
        if not m:
            continue
        pid, rest = m.groups()
        first.setdefault(pid, i)
        if rest.endswith(" <unfinished ...>"):
            pend[pid] = (rest[: -len(" <unfinished ...>")], i)
            continue
        start = i
        r = RESUMED.match(rest)
        if r:
            head, start = pend.pop(pid, ("", i))
            rest = head + r.group(2)
        c = DONE.match(rest)
        if c and c.group("name") in FORKS and re.fullmatch(r"\d+", c.group("ret")) and c.group("ret") != "0":
            args = c.group("args")
            calls.append([start, pid, c.group("nspid") or c.group("ret"), "CLONE_FILES" in args or "CLONE_THREAD" in args, False])
    out = {}
    for call in calls:
        if call[2] in first and call[2] != call[1]:
            out[call[2]] = (call[1], call[3], True)
            call[4] = True
    left = sorted((c for c in calls if not c[4]), key=lambda c: int(c[2]))
    kids = sorted((p for p, at in first.items() if p not in out and at != min(first.values())), key=int)
    if left and len(left) == len(kids) and all(c[0] < first[k] for c, k in zip(left, kids)):
        for c, k in zip(left, kids):
            out[k] = (c[1], c[3], False)
            c[4] = True
    for pid, at in sorted(first.items(), key=lambda kv: kv[1]):
        if pid in out or at == min(first.values()):
            continue
        cand = [c for c in calls if not c[4] and c[0] < at and c[1] != pid]
        if len(cand) == 1:
            out[pid] = (cand[0][1], cand[0][3], False)
            cand[0][4] = True
    return out


def parse(text, fx, sut_exec=None, controller_end=None):
    """Return (events, receipt) for one strace -f -yy trace."""
    p = Parser(fx, sut_exec)
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    elif lines:
        p.gaps.append({"reason": "trace_not_newline_terminated", "line": len(lines)})
    p.parents = fork_parents(lines)
    for i, raw in enumerate(lines, 1):
        p.line(raw, i)
    return p.events, p.receipt(controller_end)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--binding", required=True)
    ap.add_argument("--case", action="append", required=True, help="ID=TRACE")
    ap.add_argument("--principal", required=True)
    ap.add_argument("--requests-sent", type=int, required=True)
    ap.add_argument("--fx", action="append", required=True)
    ap.add_argument("--sut-exec")
    ap.add_argument("--controller-end")
    ap.add_argument("-o", "--out", required=True)
    a = ap.parse_args(argv)
    cases = []
    try:
        braw = Path(a.binding).read_bytes()
        for spec in a.case:
            cid, sep, trace = spec.partition("=")
            if not sep or not cid:
                raise Unusable(f"--case {spec}: expected ID=TRACE")
            events, receipt = parse(Path(trace).read_text(errors="replace"), a.fx, a.sut_exec, a.controller_end)
            cases.append({"case_id": cid, "principal": a.principal, "requests_sent": a.requests_sent,
                          "receipt": receipt, "events": events})
    except (Unusable, OSError) as e:
        print(f"strace.py: {e}", file=sys.stderr)
        return 2
    doc = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(braw).hexdigest(),
           "collectors": [{"name": "strace-normalize", "version": "0.1.0"}], "cases": cases}
    Path(a.out).write_text(json.dumps(doc, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
