"""Hidden oracle for eval/todo/spec.md: runs random command sessions natively
(launch + unshare, see launch.c) and compares stdout, stderr, exit status and the
file after every command with an independent Python model of the spec.

usage: python3 oracle_todo.py <image.bin> [--sessions N] [--seed S]   -> JSON on stdout
"""
import json, os, random, struct, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
LAUNCH = os.path.join(HERE, "launch")
USAGE = b"usage: todo add TEXT | list | done ID | rm ID | clear\n"
REC = 72


def pack(recs):
    out = b""
    for rid, done, text in recs:
        out += struct.pack("<IBB2x", rid, done, len(text)) + text + b"\0" * (64 - len(text))
    return out


def unpack(b):
    return [(struct.unpack_from("<I", b, i)[0], b[i + 4], b[i + 8:i + 8 + b[i + 5]]) for i in range(0, len(b), REC)]


def parse_id(s):
    if not (1 <= len(s) <= 10) or not all(48 <= c <= 57 for c in s):
        return None
    v = int(s)
    return v if v <= 0xFFFFFFFF else None


def model(args, db):
    """db: bytes or None (missing). Returns (code, out, err, new_db)."""
    def err(msg, code=1):
        return code, b"", msg, db
    recs = unpack(db) if db is not None else []
    if len(args) == 2 and args[0] == b"add":
        t = args[1]
        if not (1 <= len(t) <= 64):
            return err(b"error: bad text\n")
        if len(recs) >= 50:
            return err(b"error: full\n")
        nid = recs[-1][0] + 1 if recs else 1
        return 0, b"added %d\n" % nid, b"", pack(recs + [(nid, 0, t)])
    if len(args) == 1 and args[0] == b"list":
        if not recs:
            return 0, b"no items\n", b"", db
        return 0, b"".join(b"%d [%s] %s\n" % (r, b"x" if d else b" ", t) for r, d, t in recs), b"", db
    if len(args) == 2 and args[0] in (b"done", b"rm"):
        v = parse_id(args[1])
        if v is None:
            return err(b"error: bad id\n")
        if not any(r == v for r, _, _ in recs):
            return err(b"error: no such item\n")
        if args[0] == b"rm":
            return 0, b"removed %d\n" % v, b"", pack([x for x in recs if x[0] != v])
        return 0, b"done %d\n" % v, b"", pack([(r, 1 if r == v else d, t) for r, d, t in recs])
    if len(args) == 1 and args[0] == b"clear":
        if db is None:
            return 0, b"cleared 0\n", b"", None
        keep = [x for x in recs if not x[1]]
        return 0, b"cleared %d\n" % (len(recs) - len(keep)), b"", pack(keep)
    return err(USAGE, 2)


def rtext(rng, n=None):
    if n is None:
        n = rng.choice([0, 1, 2, 5, 10, 63, 64, 65, 70, rng.randrange(1, 65)])
    return bytes(rng.randrange(0x20, 0x7F) for _ in range(n))


def rdb(rng):
    if rng.random() < 0.2:
        return None
    n = rng.choice([0, 1, 2, 3, 5, 10, 49, 50, rng.randrange(0, 51)])
    rid = rng.randrange(1, 70000) if rng.random() < 0.7 else rng.choice([1, 0xFFFFFF00, 4294967000])
    recs = []
    for _ in range(n):
        if rid > 0xFFFFFFFF:
            break
        recs.append((rid, rng.random() < 0.4, rtext(rng, rng.randrange(1, 65))))
        rid += rng.choice([1, 1, 2, 7, 1000])
    return pack(recs)


def rcmd(rng, db):
    recs = unpack(db) if db else []
    ids = [r for r, _, _ in recs]
    def idarg():
        k = rng.random()
        if ids and k < 0.5:
            return str(rng.choice(ids)).encode()
        if ids and k < 0.6:
            return b"00" + str(rng.choice(ids)).encode()
        return rng.choice([b"0", b"1", b"", b"x", b"-1", b"+1", b"1 ", b"12345678901", b"4294967295", b"4294967296",
                           b"0000000001", b"00000000001", str(rng.randrange(1, 100000)).encode()])
    c = rng.random()
    if c < 0.3:
        return [b"add", rtext(rng)]
    if c < 0.45:
        return [b"list"]
    if c < 0.6:
        return [b"done", idarg()]
    if c < 0.75:
        return [b"rm", idarg()]
    if c < 0.85:
        return [b"clear"]
    return rng.choice([[], [b"frob"], [b""], [b"add"], [b"list", b"x"], [b"done"], [b"clear", b"1"], [b"add", b"a", b"b"], [b"ADD", b"x"], [b"rm", b"1", b"2"]])


def run(image, work, args, base=None):
    try:
        p = subprocess.run(["unshare", "-Urn", LAUNCH] + (["-b", base] if base else []) + [image, work] + [a.decode("latin-1") for a in args],
                           capture_output=True, timeout=10)
    except subprocess.TimeoutExpired:
        return None, b"", b"timeout"
    return p.returncode, p.stdout, p.stderr


def judge(image, sessions=300, seed=None, base=None):
    """base: map the image there ("f0000" for a flattened linked image, see ../todo_mod/flatten.py)."""
    seed = seed if seed is not None else random.SystemRandom().randrange(2**63)
    rng = random.Random(seed)
    fails, cmds = [], 0
    for s in range(sessions):
        db = rdb(rng)
        with tempfile.TemporaryDirectory() as work:
            path = os.path.join(work, "todo.db")
            if db is not None:
                open(path, "wb").write(db)
            for step in range(rng.randrange(1, 7)):
                args = rcmd(rng, db)
                # Keep ids inside 32 bits for add (the spec does not define the overflow).
                if args[:1] == [b"add"] and db and unpack(db) and unpack(db)[-1][0] == 0xFFFFFFFF:
                    args = [b"list"]
                code, out, err, ndb = model(args, db)
                rc, o, e = run(image, work, args, base)
                got = open(path, "rb").read() if os.path.exists(path) else None
                cmds += 1
                bad = []
                if rc != code:
                    bad.append(f"exit {rc} want {code}")
                if o != out:
                    bad.append(f"stdout {o[:80]!r} want {out[:80]!r}")
                if e != err:
                    bad.append(f"stderr {e[:80]!r} want {err[:80]!r}")
                if got != ndb:
                    bad.append("file " + ("missing" if got is None else f"{len(got)} bytes") + " want " + ("missing" if ndb is None else f"{len(ndb)} bytes"))
                if bad:
                    fails.append({"session": s, "step": step, "args": [a.decode("latin-1") for a in args], "records_before": None if db is None else len(db) // REC, "problems": bad})
                    break
                db = ndb
    return {"image": image, "seed": seed, "sessions": sessions, "commands": cmds, "failed_sessions": len(fails),
            "verdict": "PASS" if not fails else "FAIL", "first_failures": fails[:5]}


if __name__ == "__main__":
    a = sys.argv[1:]
    n = int(a[a.index("--sessions") + 1]) if "--sessions" in a else 300
    sd = int(a[a.index("--seed") + 1]) if "--seed" in a else None
    b = a[a.index("--base") + 1] if "--base" in a else None
    print(json.dumps(judge(a[0], n, sd, b)))
