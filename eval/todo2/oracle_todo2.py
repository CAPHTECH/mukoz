"""Hidden oracle for eval/todo2/spec.md: random multi-command sessions run natively
(../todo/launch under unshare), compared with an independent Python model.
usage: python3 oracle_todo2.py <image.bin> [--sessions N] [--seed S]   -> JSON on stdout"""
import json, os, random, struct, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
LAUNCH = os.path.join(HERE, "..", "todo", "launch")
USAGE = b"usage: todo add [-p N] TEXT | list [open|done] | find WORD | edit ID TEXT | pri ID N | done ID | undo ID | rm ID | clear | stats | import\n"
REC = 72


def pack(recs):
    return b"".join(struct.pack("<IBBBx", i, d, p, len(t)) + t + b"\0" * (64 - len(t)) for i, d, p, t in recs)


def unpack(b):
    return [[struct.unpack_from("<I", b, k)[0], b[k + 4], b[k + 5], b[k + 8:k + 8 + b[k + 6]]] for k in range(0, len(b), REC)]


def text_ok(t):
    return 1 <= len(t) <= 64


def id_val(s):
    if not (1 <= len(s) <= 10) or not all(48 <= c <= 57 for c in s):
        return None
    v = int(s)
    return v if v <= 0xFFFFFFFF else None


def line(r):
    return b"%d [%s] p%d %s\n" % (r[0], b"x" if r[1] else b" ", r[2], r[3])


def model(args, db, stdin):
    """db: bytes or None. Returns (code, out, err, new_db)."""
    def err(m, c=1):
        return c, b"", m, db
    recs = unpack(db) if db is not None else []
    nid = lambda rs: rs[-1][0] + 1 if rs else 1
    a = args
    cmd, rest = (a[0], a[1:]) if a else (None, [])
    forms = {b"add": (1, 3), b"list": (0, 1), b"find": (1,), b"edit": (2,), b"pri": (2,), b"done": (1,), b"undo": (1,), b"rm": (1,),
             b"clear": (0,), b"stats": (0,), b"import": (0,)}
    if cmd not in forms or len(rest) not in forms[cmd] or (cmd == b"add" and len(rest) == 3 and rest[0] != b"-p") \
            or (cmd == b"list" and len(rest) == 1 and rest[0] not in (b"open", b"done")):
        return err(USAGE, 2)
    if cmd == b"add":
        t = rest[-1]
        if not text_ok(t):
            return err(b"error: bad text\n")
        p = 2
        if len(rest) == 3:
            if rest[1] not in (b"1", b"2", b"3"):
                return err(b"error: bad priority\n")
            p = int(rest[1])
        if len(recs) >= 50:
            return err(b"error: full\n")
        i = nid(recs)
        return 0, b"added %d\n" % i, b"", pack(recs + [[i, 0, p, t]])
    if cmd == b"list":
        sel = [r for r in recs if not rest or (rest[0] == b"open" and not r[1]) or (rest[0] == b"done" and r[1])]
        sel.sort(key=lambda r: (r[2], r[0]))
        return 0, (b"".join(map(line, sel)) or b"no items\n"), b"", db
    if cmd == b"find":
        w = rest[0]
        if not text_ok(w):
            return err(b"error: bad text\n")
        sel = [r for r in recs if w.lower() in r[3].lower()]
        return 0, (b"".join(map(line, sel)) or b"no match\n"), b"", db
    if cmd in (b"edit", b"pri", b"done", b"undo", b"rm"):
        if cmd == b"edit" and not text_ok(rest[1]):
            return err(b"error: bad text\n")
        if cmd == b"pri" and rest[1] not in (b"1", b"2", b"3"):
            return err(b"error: bad priority\n")
        v = id_val(rest[0])
        if v is None:
            return err(b"error: bad id\n")
        k = next((k for k, r in enumerate(recs) if r[0] == v), None)
        if k is None:
            return err(b"error: no such item\n")
        r = recs[k]
        if cmd == b"edit":
            r[3] = rest[1]; out = b"edited %d\n" % v
        elif cmd == b"pri":
            r[2] = int(rest[1]); out = b"pri %d %s\n" % (v, rest[1])
        elif cmd == b"done":
            r[1] = 1; out = b"done %d\n" % v
        elif cmd == b"undo":
            r[1] = 0; out = b"undone %d\n" % v
        else:
            del recs[k]; out = b"removed %d\n" % v
        return 0, out, b"", pack(recs)
    if cmd == b"clear":
        if db is None:
            return 0, b"cleared 0\n", b"", None
        keep = [r for r in recs if not r[1]]
        return 0, b"cleared %d\n" % (len(recs) - len(keep)), b"", pack(keep)
    if cmd == b"stats":
        d = sum(1 for r in recs if r[1])
        return 0, b"total %d open %d done %d\n" % (len(recs), len(recs) - d, d), b"", db
    # import
    lines = stdin.split(b"\n")
    if stdin.endswith(b"\n"):
        lines = lines[:-1]
    for k, l in enumerate(lines, 1):
        if len(l) > 64:
            return err(b"error: bad line %d\n" % k)
    new = [l for l in lines if l]
    if len(recs) + len(new) > 50:
        return err(b"error: full\n")
    if not new:
        return 0, b"imported 0\n", b"", db
    for l in new:
        recs.append([nid(recs), 0, 2, l])
    return 0, b"imported %d\n" % len(new), b"", pack(recs)


def rtext(rng, n=None):
    if n is None:
        n = rng.choice([0, 1, 2, 5, 10, 63, 64, 65, 70, rng.randrange(1, 65), rng.randrange(1, 65)])
    return bytes(rng.choice(b"abcXYZ  ") if rng.random() < 0.5 else rng.randrange(0x20, 0x7F) for _ in range(n))


def rdb(rng):
    if rng.random() < 0.15:
        return None
    n = rng.choice([0, 1, 2, 3, 5, 10, 48, 49, 50, rng.randrange(0, 51)])
    rid = rng.randrange(1, 70000) if rng.random() < 0.7 else rng.choice([1, 3999999000])
    recs = []
    for _ in range(n):
        if rid > 4000000000:
            break
        recs.append([rid, int(rng.random() < 0.4), rng.choice([1, 2, 3]), rtext(rng, rng.randrange(1, 65))])
        rid += rng.choice([1, 1, 2, 7, 1000])
    return pack(recs)


def rstdin(rng):
    k = rng.random()
    if k < 0.3:
        return b""
    ls = [rtext(rng, rng.choice([0, 0, 1, 5, 30, 64, 65, rng.randrange(1, 65)])) for _ in range(rng.choice([1, 2, 3, 5, 10, 30, 52]))]
    s = b"\n".join(ls)
    if rng.random() < 0.6:
        s += b"\n"
    return s[:8192]


def rcmd(rng, db):
    recs = unpack(db) if db else []
    ids = [r[0] for r in recs]
    words = [r[3][i:i + rng.randrange(1, 6)] for r in recs for i in [rng.randrange(0, len(r[3]))]] or [b"a"]
    def idarg():
        k = rng.random()
        if ids and k < 0.6:
            return str(rng.choice(ids)).encode()
        if ids and k < 0.7:
            return b"00" + str(rng.choice(ids)).encode()
        return rng.choice([b"0", b"1", b"", b"x", b"-1", b"12345678901", b"4294967295", b"4294967296", b"0000000001", str(rng.randrange(1, 100000)).encode()])
    pr = lambda: rng.choice([b"1", b"2", b"3", b"1", b"2", b"3", b"0", b"4", b"", b"01", b"x", b"12"])
    c = rng.randrange(14)
    if c == 0:
        return [b"add", rtext(rng)]
    if c == 1:
        return [b"add", b"-p", pr(), rtext(rng)]
    if c == 2:
        return [b"list"] + rng.choice([[], [b"open"], [b"done"]])
    if c == 3:
        w = rng.choice(words)
        w = w.swapcase() if rng.random() < 0.5 else w
        return [b"find", rng.choice([w, w, rtext(rng, rng.choice([0, 1, 2, 65]))])]
    if c == 4:
        return [b"edit", idarg(), rtext(rng)]
    if c == 5:
        return [b"pri", idarg(), pr()]
    if c in (6, 7):
        return [rng.choice([b"done", b"undo", b"rm"]), idarg()]
    if c == 8:
        return [b"clear"]
    if c == 9:
        return [b"stats"]
    if c in (10, 11):
        return [b"import"]
    return rng.choice([[], [b"frob"], [b""], [b"add"], [b"add", b"-p", b"1"], [b"add", b"-q", b"1", b"x"], [b"add", b"x", b"y"], [b"list", b"all"],
                       [b"list", b"open", b"x"], [b"find"], [b"edit", b"1"], [b"pri", b"1"], [b"done"], [b"clear", b"1"], [b"stats", b"x"],
                       [b"import", b"x"], [b"LIST"], [b"list", b"OPEN"]])


def run(image, work, args, stdin):
    try:
        p = subprocess.run(["unshare", "-Urn", LAUNCH, image, work] + [x.decode("latin-1") for x in args], input=stdin,
                           capture_output=True, timeout=10)
    except subprocess.TimeoutExpired:
        return None, b"", b"timeout"
    return p.returncode, p.stdout, p.stderr


def judge(image, sessions=300, seed=None):
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
                stdin = rstdin(rng)
                code, out, err, ndb = model(args, db, stdin)
                rc, o, e = run(image, work, args, stdin)
                got = open(path, "rb").read() if os.path.exists(path) else None
                cmds += 1
                bad = []
                if rc != code: bad.append(f"exit {rc} want {code}")
                if o != out: bad.append(f"stdout {o[:100]!r} want {out[:100]!r}")
                if e != err: bad.append(f"stderr {e[:80]!r} want {err[:80]!r}")
                if got != ndb:
                    bad.append("file " + ("missing" if got is None else f"{len(got)} bytes") + " want " + ("missing" if ndb is None else f"{len(ndb)} bytes")
                               + ("" if got is None or ndb is None else f" (first diff at {next((i for i in range(min(len(got), len(ndb))) if got[i] != ndb[i]), min(len(got), len(ndb)))})"))
                if bad:
                    fails.append({"session": s, "step": step, "args": [x.decode("latin-1") for x in args], "stdin_bytes": len(stdin),
                                  "records_before": None if db is None else len(db) // REC, "problems": bad})
                    break
                db = ndb
    return {"image": image, "seed": seed, "sessions": sessions, "commands": cmds, "failed_sessions": len(fails),
            "verdict": "PASS" if not fails else "FAIL", "first_failures": fails[:5]}


if __name__ == "__main__":
    a = sys.argv[1:]
    n = int(a[a.index("--sessions") + 1]) if "--sessions" in a else 300
    sd = int(a[a.index("--seed") + 1]) if "--seed" in a else None
    print(json.dumps(judge(a[0], n, sd)))
