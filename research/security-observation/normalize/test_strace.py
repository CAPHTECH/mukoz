#!/usr/bin/env python3
"""Negative tests for the strace normalizer, on recorded E0 traces and small synthetic ones.

usage: test_strace.py                  run against ./strace.py
       NORMALIZER=<file> test_strace.py   run against another copy (mutate.py uses this)
"""
import hashlib
import importlib.util
import json
import os
import re
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


S = load("strace_norm", Path(os.environ.get("NORMALIZER", HERE / "strace.py")))
A = load("adapter", ROOT / "adapter" / "adapter.py")
BRAW = (HERE / "e0_binding.json").read_bytes()
BINDING = json.loads(BRAW)
R3, S6 = ROOT / "e0" / "runs" / "r3", ROOT / "sbx" / "runs" / "s6"
FX = ["/mnt/fx/", "/srv/fx/"]
P = "external.sec."
OPEN, READ, WRITE = P + "fs.no_forbidden_open", P + "fs.no_forbidden_read", P + "fs.no_forbidden_write"
ATTEMPT, CONNECT, EXEC = P + "net.no_forbidden_attempt", P + "net.no_forbidden_connect", P + "proc.allowed_exec_only"


def claims(text, sut_exec="./cal", end=None):
    events, receipt = S.parse(text, FX, sut_exec, end)
    case = {"case_id": "c", "principal": "cal", "requests_sent": 1, "receipt": receipt, "events": events}
    o = {"schema": "secobs.observations/0", "binding_sha256": hashlib.sha256(BRAW).hexdigest(), "cases": [case]}
    out, _ = A.convert(BRAW, BINDING, json.dumps(o).encode(), o, "0" * 64, "independent")
    return {c["property"]: (c["eval"], c["reason"]) for c in out["claims"]}


def trace(run, root=R3):
    return (root / run / "strace.txt").read_text()


def sub(text, pattern, repl, count=1):
    new, n = re.subn(pattern, repl, text, count=count, flags=re.M)
    assert n == count, (pattern, n)
    return new


def syn(*body, start='100 execve("./cal", [...], 0x1 /* 1 vars */) = 0', end="100 exit_group(0)                     = ?"):
    return "\n".join([start, *body, end]) + "\n"


SAT, VIO, INC = A.SAT, A.VIO, A.INC


class RecordedBaseline(unittest.TestCase):
    def test_fdreuse(self):
        c = claims(trace("strace-fdreuse"))
        self.assertEqual(c[READ], (VIO, None))
        self.assertEqual(c[WRITE], (SAT, None))

    def test_normal_is_satisfied(self):
        self.assertTrue(all(v == (SAT, None) for v in claims(trace("strace-normal")).values()))


class ReturnValues(unittest.TestCase):
    """7.5 at the B layer: changing a positive return makes the violation disappear."""

    def test_read_positive_to_failed(self):
        t = sub(trace("strace-fdreuse"), r"^(\d+ read\(\d+</mnt/fx/secret>, .*\) += )10$", r"\g<1>-1 EBADF (Bad file descriptor)")
        self.assertEqual(claims(t)[READ], (SAT, None))
        self.assertEqual(claims(t)[OPEN], (VIO, None))

    def test_read_positive_to_zero(self):
        t = sub(trace("strace-fdreuse"), r"^(\d+ read\(\d+</mnt/fx/secret>, .*\) += )10$", r"\g<1>0")
        self.assertEqual(claims(t)[READ], (SAT, None))

    def test_sendfile_positive_and_zero(self):
        body = ['100 openat(AT_FDCWD</x>, "/mnt/fx/secret", O_RDONLY) = 3</mnt/fx/secret>',
                '100 openat(AT_FDCWD</x>, "/mnt/fx/out/sf", O_WRONLY|O_CREAT|O_TRUNC, 0644) = 4</mnt/fx/out/sf>']
        sf = "100 sendfile(4</mnt/fx/out/sf>, 3</mnt/fx/secret>, NULL, 4096) = {}"
        self.assertEqual(claims(syn(*body, sf.format(4096)))[READ], (VIO, None))
        self.assertEqual(claims(syn(*body, sf.format(0)))[READ], (SAT, None))

    def test_copy_file_range_and_splice_count_the_source(self):
        t = trace("strace-kxfer")
        t = sub(t, r"^(\d+ sendfile\(.*\) += )4096$", r"\g<1>0")
        self.assertEqual(claims(t)[READ], (VIO, None))   # copy_file_range and splice still read secret
        t = sub(t, r"^(\d+ copy_file_range\(.*\) += )4096$", r"\g<1>0")
        t = sub(t, r"^(\d+ splice\(3</mnt/fx/secret>.*\) += )4096$", r"\g<1>0")
        self.assertEqual(claims(t)[READ], (SAT, None))


class LogLoss(unittest.TestCase):
    """Log loss and parser failure never become success; a counterexample already seen stays."""

    def assert_none_sat(self, c):
        self.assertFalse([k for k, v in c.items() if v[0] == SAT], c)

    def test_truncated_mid_line(self):
        t = trace("strace-normal")
        self.assert_none_sat(claims(t[: t.rindex("exit_group") + 5]))

    def test_sut_exec_missing(self):
        self.assert_none_sat(claims(sub(trace("strace-normal"), r'^\d+ execve\("\./cal".*\n', "")))

    def test_sut_exit_missing(self):
        self.assert_none_sat(claims(sub(trace("strace-normal"), r"^\d+ exit_group\(0\) .*\n", "")))

    def test_garbage_line(self):
        t = trace("strace-normal").replace("\n", "\n#### garbage\n", 5)
        self.assert_none_sat(claims(t))

    def test_strace_message(self):
        t = trace("strace-normal").replace("\n", "\nstrace: Process 1 detached\n", 1)
        self.assert_none_sat(claims(t))

    def test_resumed_line_lost(self):
        t = sub(trace("strace-child"), r"^\d+ <\.\.\. clone3 resumed>.*\n", "")
        self.assert_none_sat(claims(t))

    def test_unknown_return_on_io(self):
        t = sub(trace("strace-normal"), r"^(\d+ read\(\d+</mnt/fx/public>, .*\) += )\d+$", r"\g<1>?")
        self.assertEqual(claims(t)[READ][0], INC)

    def test_restart_syscall(self):
        t = syn("100 restart_syscall(<... resuming interrupted read ...>) = 0")
        self.assertEqual(claims(t)[READ], (INC, "MONITORING_INCOMPLETE"))

    def test_resumed_with_other_name(self):
        t = syn("100 read(3</mnt/fx/public>,  <unfinished ...>", '100 <... write resumed>"", 64) = 64')
        self.assertEqual(claims(t)[READ], (INC, "MONITORING_INCOMPLETE"))

    def test_unfinished_never_resumed(self):
        t = syn("100 read(3</mnt/fx/public>,  <unfinished ...>")
        self.assertEqual(claims(t)[READ], (INC, "MONITORING_INCOMPLETE"))

    def test_no_final_newline(self):
        self.assertEqual(claims(syn().rstrip("\n"))[READ], (INC, "MONITORING_INCOMPLETE"))

    def test_violation_survives_later_loss(self):
        t = trace("strace-fdreuse")
        t = t[: t.rindex("exit_group")] + "exit_gr"
        c = claims(t)
        self.assertEqual(c[READ], (VIO, None))
        self.assertEqual(c[WRITE][0], INC)

    def test_forced_stop(self):
        self.assert_none_sat(claims(syn(end="100 +++ killed by SIGKILL +++")))

    def test_crash_is_an_observed_end(self):
        self.assertEqual(claims(syn(end="100 +++ killed by SIGSEGV +++"))[READ], (SAT, None))

    def test_controller_says_not_drained(self):
        self.assert_none_sat(claims(syn(), end="timeout"))


class Identity(unittest.TestCase):
    """Resource identity from the decoration, cross-checked with the FD lineage (9.2, 9.3)."""

    OPEN3 = '100 openat(AT_FDCWD</x>, "/mnt/fx/secret", O_RDONLY{}) = 3</mnt/fx/secret>'

    def test_lineage_fills_missing_decoration(self):
        c = claims(syn(self.OPEN3.format(""), "100 read(3, \"\"..., 64) = 64"))
        self.assertEqual(c[READ], (VIO, None))

    def test_no_decoration_and_no_lineage_is_unresolved(self):
        c = claims(syn("100 read(3, \"\"..., 64) = 64"))
        self.assertEqual(c[READ], (INC, "ATTRIBUTION_UNRESOLVED"))

    def test_decoration_conflicting_with_lineage_is_unresolved(self):
        c = claims(syn(self.OPEN3.format(""), "100 read(3</mnt/fx/public>, \"\"..., 64) = 64"))
        self.assertEqual(c[READ], (INC, "ATTRIBUTION_UNRESOLVED"))

    def test_dup2_carries_identity(self):
        c = claims(syn(self.OPEN3.format(""), "100 dup2(3, 7) = 7", "100 close(3) = 0", "100 read(7, \"\"..., 64) = 64"))
        self.assertEqual(c[READ], (VIO, None))

    def test_close_on_exec_drops_identity(self):
        execve = '100 execve("/bin/cat", [...], 0x1 /* 1 vars */) = 0'
        kept = claims(syn(self.OPEN3.format(""), execve, "100 read(3, \"\"..., 64) = 64"))
        dropped = claims(syn(self.OPEN3.format("|O_CLOEXEC"), execve, "100 read(3, \"\"..., 64) = 64"))
        self.assertEqual(kept[READ], (VIO, None))
        self.assertEqual(dropped[READ], (INC, "ATTRIBUTION_UNRESOLVED"))

    def test_child_inherits_fds(self):
        c = claims(syn(self.OPEN3.format(""),
                       "100 clone(child_stack=NULL, flags=SIGCHLD) = 101",
                       "101 read(3, \"\"..., 64) = 64", "101 exit_group(0) = ?"))
        self.assertEqual(c[READ], (VIO, None))

    def test_symlink_uses_resolved_target(self):
        c = claims(syn('100 openat(AT_FDCWD</x>, "/mnt/fx/link", O_RDONLY) = 3</mnt/fx/secret>',
                       "100 read(3</mnt/fx/secret>, \"\"..., 100) = 100"))
        self.assertEqual(c[READ], (VIO, None))

    def test_deleted_file_is_unresolved(self):
        c = claims(syn("100 read(3</mnt/fx/secret (deleted)>, \"\"..., 100) = 100"))
        self.assertEqual(c[READ], (INC, "ATTRIBUTION_UNRESOLVED"))

    def test_o_path_is_not_a_readable_open(self):
        c = claims(syn('100 openat(AT_FDCWD</x>, "/mnt/fx/secret", O_RDONLY|O_PATH) = 3</mnt/fx/secret>'))
        self.assertEqual(c[OPEN], (SAT, None))


class Processes(unittest.TestCase):
    def test_descendant_in_pid_namespace(self):
        c = claims(trace("ns-strace-child"))
        self.assertEqual(c[READ], (VIO, None))

    def test_unmatched_child_is_uncertain(self):
        t = trace("ns-strace-child")
        t = re.sub(r"^\d+ clone\(.*\n", "", t, flags=re.M)
        t = re.sub(r"^\d+ clone3\(.*\n", "", t, flags=re.M)
        t = re.sub(r"^\d+ <\.\.\. clone3 resumed>.*\n", "", t, flags=re.M)
        self.assertEqual(claims(t)[READ], (INC, "ATTRIBUTION_UNRESOLVED"))

    def test_pidns_translation_comment(self):
        c = claims(syn("100 clone(child_stack=NULL, flags=SIGCHLD) = 2 /* 777 in strace's PID NS */",
                       '777 openat(AT_FDCWD</x>, "/mnt/fx/secret", O_RDONLY) = 3</mnt/fx/secret>',
                       "777 exit_group(0) = ?"))
        self.assertEqual(c[OPEN], (VIO, None))

    def test_supervisor_before_sut_exec_is_out_of_scope(self):
        t = trace("sbx-strace-noop", S6)
        first = t.split(" ", 1)[0]
        t = f'{first} openat(AT_FDCWD</x>, "/srv/fx/secret", O_RDONLY) = 3</srv/fx/secret>\n' + t
        c = claims(t, "/sut/")
        self.assertEqual(c[OPEN], (SAT, None))

    def test_sut_in_sandbox_reading_secret(self):
        self.assertEqual(claims(trace("sbx-strace-fdreuse", S6), "/sut/")[READ], (VIO, None))

    def test_grace0_kill_is_not_success(self):
        c = claims(trace("sbx-grace0-late", S6), "/sut/", "grace_expired")
        self.assertFalse([k for k, v in c.items() if v[0] == SAT])

    def test_unauthorized_exec(self):
        c = claims(syn('100 execve("/bin/sh", [...], 0x1 /* 1 vars */) = 0'))
        self.assertEqual(c[EXEC], (VIO, None))

    def test_failed_exec_is_not_violation(self):
        c = claims(syn('100 execve("/bin/sh", [...], 0x1 /* 1 vars */) = -1 ENOENT (No such file or directory)'))
        self.assertEqual(c[EXEC], (SAT, None))


class Network(unittest.TestCase):
    CON = '100 connect(3<TCP:[1]>, {{sa_family=AF_INET, sin_port=htons(80), sin_addr=inet_addr("10.77.0.3")}}, 16) = {}'

    def test_refused_is_attempt_only(self):
        c = claims(syn(self.CON.format("-1 ECONNREFUSED (Connection refused)")))
        self.assertEqual((c[ATTEMPT][0], c[CONNECT][0]), (VIO, SAT))

    def test_einprogress_is_not_established(self):
        # Not established, and not "not established" either: the outcome is not followed (E1 smoke, impl 2).
        c = claims(syn(self.CON.format("-1 EINPROGRESS (Operation now in progress)")))
        self.assertEqual((c[ATTEMPT][0], c[CONNECT]), (VIO, (INC, "OUTCOME_UNOBSERVED")))

    def test_established(self):
        c = claims(syn(self.CON.format("0")))
        self.assertEqual(c[CONNECT], (VIO, None))

    def test_allowed_receiver(self):
        c = claims(syn(self.CON.format("0").replace("10.77.0.3", "10.77.0.1").replace("htons(80)", "htons(8080)")))
        self.assertEqual((c[ATTEMPT][0], c[CONNECT][0]), (SAT, SAT))


class Unsupported(unittest.TestCase):
    """Mechanisms the first B does not follow are recorded, never silently allowed (9.2)."""

    def test_mmap_of_protected_file(self):
        c = claims(syn('100 openat(AT_FDCWD</x>, "/mnt/fx/rw", O_RDONLY) = 3</mnt/fx/rw>',
                       "100 mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_SHARED, 3</mnt/fx/rw>, 0) = 0x7f0000000000"))
        self.assertEqual(c[WRITE], (INC, "UNSUPPORTED_MECHANISM"))

    def test_mmap_of_library_is_ignored(self):
        c = claims(syn("100 mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, 3</usr/lib/libc.so.6>, 0) = 0x7f0000000000"))
        self.assertEqual(c[READ], (SAT, None))

    def test_io_uring(self):
        c = claims(syn("100 io_uring_setup(8, {flags=0}) = 3<anon_inode:[io_uring]>"))
        self.assertEqual(c[READ], (INC, "UNSUPPORTED_MECHANISM"))

    def test_rename_of_protected_file(self):
        c = claims(syn('100 renameat2(AT_FDCWD</x>, "/mnt/fx/out/new", AT_FDCWD</x>, "/mnt/fx/rw", 0) = 0'))
        self.assertEqual(c[WRITE][0], INC)


if __name__ == "__main__":
    unittest.main()
