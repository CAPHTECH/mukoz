"""Hidden final judgement for the comparison experiment (docs/11 11.2).

Independent of Mukoz: Python references written by hand, a C runner executing
the submitted code natively on this x86-64 host. Cases are generated with a
seed chosen at judgement time, after the submission exists.

usage: python3 oracle.py <task> <code.bin> [--cases N] [--seed S]
"""
import json, os, random, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

HERE = os.path.dirname(os.path.abspath(__file__))
M64 = (1 << 64) - 1

def s64(x):
    return x - (1 << 64) if x >> 63 else x

def garbage_upper(rng, v, bits):
    return (rng.getrandbits(64) & ~((1 << bits) - 1) & M64) | v

def interesting(rng, bits):
    m = (1 << bits) - 1
    pool = [0, 1, 2, 3, m, m - 1, m >> 1, (m >> 1) + 1, 0x80, 0x7f, 0xff, 0x100, 0xffff, 0x10000, 0xffffffff, 0x100000000]
    r = rng.random()
    if r < 0.35:
        return rng.choice(pool) & m
    if r < 0.5:
        return rng.randrange(0, 64) & m
    if r < 0.6:
        return (m - rng.randrange(0, 64)) & m
    return rng.getrandbits(bits)

def rbytes(rng, n, nonzero=False):
    if nonzero:
        return bytes(rng.randrange(1, 256) for _ in range(n))
    if rng.random() < 0.3:
        alphabet = [0, 1, 0x41, 0xff, rng.randrange(256)]
        return bytes(rng.choice(alphabet) for _ in range(n))
    return bytes(rng.randrange(256) for _ in range(n))

def rlen(rng):
    r = rng.random()
    if r < 0.15: return 0
    if r < 0.3: return rng.randrange(1, 4)
    if r < 0.4: return 256
    return rng.randrange(0, 257)

# Each task: gen(rng) -> (bufs, args, expect) ; expect(result) -> None | message
def t_abs_diff(rng):
    a, b = interesting(rng, 64), interesting(rng, 64)
    want = (a - b) if a >= b else (b - a)
    return [], [a, b], lambda r: None if r["rax"] == want else f"rax={r['rax']:#x} want {want:#x} (a={a:#x}, b={b:#x})"

def t_smax(rng):
    a, b = interesting(rng, 64), interesting(rng, 64)
    want = a if s64(a) >= s64(b) else b
    return [], [a, b], lambda r: None if r["rax"] == want else f"rax={r['rax']:#x} want {want:#x}"

def t_popcount(rng):
    x = interesting(rng, 64)
    want = bin(x).count("1")
    return [], [x], lambda r: None if r["rax"] == want else f"rax={r['rax']} want {want} (x={x:#x})"

def t_sat_add_u32(rng):
    a, b = interesting(rng, 32), interesting(rng, 32)
    want = min(a + b, 0xffffffff)
    return [], [garbage_upper(rng, a, 32), garbage_upper(rng, b, 32)], \
        lambda r: None if r["rax"] & 0xffffffff == want else f"eax={r['rax'] & 0xffffffff:#x} want {want:#x}"

def t_fill(rng):
    n = rlen(rng); c = rng.randrange(256); buf = rbytes(rng, n)
    want = bytes([c]) * n
    return [buf], ["p0", garbage_upper(rng, c, 8), n], \
        lambda r: None if r["bufs"][0] == want else f"dst wrong (n={n}, c={c:#x})"

def t_count_byte(rng):
    n = rlen(rng); buf = rbytes(rng, n)
    c = rng.choice(list(buf)) if n and rng.random() < 0.7 else rng.randrange(256)
    want = buf.count(c)
    return [buf], ["p0", n, garbage_upper(rng, c, 8)], \
        lambda r: None if r["rax"] == want else f"rax={r['rax']} want {want} (n={n}, c={c:#x})"

def t_reverse(rng):
    n = rlen(rng); buf = rbytes(rng, n)
    want = buf[::-1]
    return [buf], ["p0", n], lambda r: None if r["bufs"][0] == want else f"buf wrong (n={n})"

def t_checked_mul(rng):
    a, b = interesting(rng, 64), interesting(rng, 64)
    if rng.random() < 0.3:
        a, b = rng.getrandbits(32), rng.getrandbits(32)
    old = rng.getrandbits(64)
    p = a * b
    ok = p <= M64
    want_status = 0 if ok else 1
    want_out = (p if ok else old).to_bytes(8, "little")
    def check(r):
        st = r["rax"] & 0xffffffff
        if st != want_status:
            return f"eax={st} want {want_status} (a={a:#x}, b={b:#x})"
        if r["bufs"][0] != want_out:
            return f"*out={int.from_bytes(r['bufs'][0], 'little'):#x} want {int.from_bytes(want_out, 'little'):#x}"
        return None
    return [old.to_bytes(8, "little")], [a, b, "p0"], check

def t_memmove(rng):
    L = rng.choice([0, 1, 2, 8, 64, 128, rng.randrange(0, 129)])
    buf = rbytes(rng, L)
    n = rng.choice([0, L, rng.randrange(0, L + 1)])
    doff = rng.randrange(0, L - n + 1); soff = rng.randrange(0, L - n + 1)
    if rng.random() < 0.4 and L - n >= 1:
        soff = rng.randrange(0, L - n + 1); doff = min(L - n, soff + rng.randrange(0, 4))
    want = bytearray(buf); want[doff:doff + n] = buf[soff:soff + n]
    return [buf], [f"p0+{doff}", f"p0+{soff}", n], \
        lambda r: None if r["bufs"][0] == bytes(want) else f"buf wrong (L={L}, n={n}, doff={doff}, soff={soff})"

def t_hex_encode(rng):
    n = rng.choice([0, 1, 2, 128, rng.randrange(0, 129)])
    src = rbytes(rng, n); dst = rbytes(rng, 2 * n)
    want = src.hex().encode()
    return [dst, src], ["p0", "p1", n], lambda r: None if r["bufs"][0] == want else f"dst wrong (n={n})"

def t_shl_var(rng):
    x = interesting(rng, 64)
    s = rng.choice([0, 1, 31, 32, 63, 64, 65, 127, 128, 255, 256, 1 << 32, (1 << 64) - 1, rng.randrange(0, 64), rng.getrandbits(64), rng.randrange(64, 512)])
    want = (x << s) & M64 if s < 64 else 0
    return [], [x, s], lambda r: None if r["rax"] == want else f"rax={r['rax']:#x} want {want:#x} (x={x:#x}, s={s})"

def t_isqrt(rng):
    import math
    r0 = rng.getrandbits(32)
    x = rng.choice([interesting(rng, 64), r0 * r0, max(r0 * r0 - 1, 0), (r0 * r0 + 2 * r0) & M64, rng.getrandbits(64)])
    want = math.isqrt(x)
    return [], [x], lambda r: None if r["rax"] == want else f"rax={r['rax']} want {want} (x={x:#x})"

def t_base64(rng):
    import base64
    n = rng.choice([0, 1, 2, 3, 4, 5, 96, rng.randrange(0, 97)])
    src = rbytes(rng, n); dst = rbytes(rng, 4 * ((n + 2) // 3))
    want = base64.b64encode(src)
    return [dst, src], ["p0", "p1", n], lambda r: None if r["bufs"][0] == want else f"dst wrong (n={n}): {r['bufs'][0][-8:]!r} want {want[-8:]!r}"

def t_count_byte_fast(rng):
    # Adversarial bytes for word-at-a-time (SWAR) implementations: c next to c^1 etc.
    n = rlen(rng)
    c = rng.randrange(256)
    pool = [c, c ^ 1, c ^ 0x80, c ^ 0xff, 0, 1, 0x80, 0xff]
    adv = rng.random() < 0.6
    buf = bytes(rng.choice(pool) if adv and rng.random() < 0.8 else rng.randrange(256) for _ in range(n))
    want = buf.count(c)
    return [buf], ["p0", n, garbage_upper(rng, c, 8)], \
        lambda r: None if r["rax"] == want else f"rax={r['rax']} want {want} (n={n}, c={c:#x})"

def _utf8_valid_string(rng, n_cp):
    edges = [0, 0x7f, 0x80, 0x7ff, 0x800, 0xfff, 0x1000, 0xd7ff, 0xe000, 0xfffd, 0xffff, 0x10000, 0x3ffff, 0x40000, 0x10ffff]
    out = []
    for _ in range(n_cp):
        if rng.random() < 0.12:
            out.extend(chr(rng.randrange(0, 0x80)) for _ in range(rng.randrange(1, 24)))
            continue
        r = rng.random()
        if r < 0.3: cp = rng.choice(edges)
        elif r < 0.55: cp = rng.randrange(0, 0x80)
        elif r < 0.7: cp = rng.randrange(0x80, 0x800)
        elif r < 0.85: cp = rng.choice([rng.randrange(0x800, 0xd800), rng.randrange(0xe000, 0x10000)])
        else: cp = rng.randrange(0x10000, 0x110000)
        out.append(chr(cp))
    return "".join(out).encode("utf-8")

def _utf8_corrupt(rng, b):
    b = bytearray(b)
    kind = rng.randrange(9)
    pos = rng.randrange(0, len(b) + 1)
    bad = [b"\xc0\x80", b"\xc1\xbf", b"\xe0\x80\x80", b"\xe0\x9f\xbf", b"\xed\xa0\x80", b"\xed\xbf\xbf",
           b"\xf0\x80\x80\x80", b"\xf0\x8f\xbf\xbf", b"\xf4\x90\x80\x80", b"\xf5\x80\x80\x80", b"\xf8\x88\x80\x80\x80",
           b"\xff", b"\xfe", b"\x80", b"\xbf", b"\xc2", b"\xe1\x80", b"\xf1\x80\x80", b"\xc2\x41", b"\xe1\x41\x80", b"\xe1\x80\x41"]
    if kind <= 3:
        b[pos:pos] = rng.choice(bad)
    elif kind == 4 and b:
        del b[rng.randrange(len(b))]
    elif kind == 5 and b:
        b[rng.randrange(len(b))] = rng.randrange(256)
    elif kind == 6:
        b += rng.choice([b"\xc2", b"\xe1\x80", b"\xf0\x90", b"\xf0\x90\x80"])
    elif kind == 7:
        b[pos:pos] = bytes([rng.randrange(0x80, 0xc0)])
    else:
        b[pos:pos] = rng.choice(bad)
    return bytes(b)

def t_utf8_count(rng):
    n_cp = rng.choice([0, 1, 2, 3, rng.randrange(0, 40), rng.randrange(0, 120)])
    buf = _utf8_valid_string(rng, n_cp)
    if rng.random() < 0.5:
        buf = _utf8_corrupt(rng, buf)
    try:
        want = len(buf.decode("utf-8"))
    except UnicodeDecodeError:
        want = M64
    n = len(buf)
    return [buf], ["p0", n], lambda r: None if r["rax"] == want else f"rax={r['rax']:#x} want {want:#x} (n={n}, buf={buf.hex()[:80]})"

def t_utf8_to_utf16(rng):
    n_cp = rng.choice([0, 1, 2, 3, rng.randrange(0, 30), rng.randrange(0, 60)])
    src = _utf8_valid_string(rng, n_cp)
    if rng.random() < 0.4:
        src = _utf8_corrupt(rng, src)
    n = len(src)
    dst = rbytes(rng, 2 * n)
    try:
        out = src.decode("utf-8").encode("utf-16-le")
        want = len(out) // 2
    except UnicodeDecodeError:
        out, want = None, M64
    def check(r):
        if r["rax"] != want:
            return f"rax={r['rax']:#x} want {want:#x} (n={n}, src={src.hex()[:60]})"
        if out is not None:
            got = r["bufs"][0]
            if got[:len(out)] != out:
                return f"dst wrong (n={n}, src={src.hex()[:60]})"
            if got[len(out):] != dst[len(out):]:
                return f"wrote beyond the returned units (n={n})"
        return None
    return [dst, src], ["p0", "p1", n], check

def t_utf8_to_utf16_fast(rng):
    return t_utf8_to_utf16(rng)

def _b64_corrupt(rng, t):
    t = bytearray(t)
    k = rng.randrange(7)
    if k == 0 and t: t[rng.randrange(len(t))] = rng.choice(b"!*-_.= \n\x00\x80")
    elif k == 1: t += b"A" * rng.randrange(1, 4)
    elif k == 2 and len(t) >= 5: t[rng.randrange(len(t) - 4)] = ord("=")
    elif k == 3 and t.endswith(b"=") and len(t) >= 2:
        i = len(t) - (2 if t.endswith(b"==") else 1) - 1
        t[i] = ord(rng.choice("BCDEFGHIJKLMNOPQRSTUVWXYZbcdefghijklmnopqrstuvwxyz0123456789+/"))
    elif k == 4: t = t[:-1] if t else t
    elif k == 5 and len(t) >= 4: t[-4:] = rng.choice([b"A===", b"====", b"AA=A", b"=AAA"])
    else: t += rng.choice([b"====", b"A=", b"AB=="])
    return bytes(t)

def t_codec(rng):
    import base64
    op = rng.choice([0, 0, 1, 1, 2, 3, 3, rng.choice([4, 5, 0xffffffffffffffff, 1 << 32])])
    if op == 0:
        src = _utf8_valid_string(rng, rng.choice([0, 1, rng.randrange(0, 30), rng.randrange(0, 60)]))
        if rng.random() < 0.4: src = _utf8_corrupt(rng, src)
        try: want = src.decode("utf-8").encode("utf-16-le"); ret = len(want) // 2
        except UnicodeDecodeError: want, ret = None, M64
    elif op == 1:
        src = _utf8_valid_string(rng, rng.choice([0, 1, rng.randrange(0, 30), rng.randrange(0, 60)])).decode("utf-8").encode("utf-16-le")
        if rng.random() < 0.4:
            src = bytearray(src); k = rng.randrange(5); pos = rng.randrange(0, len(src) // 2 + 1) * 2
            if k == 0: src[pos:pos] = rng.choice([b"\x00\xd8", b"\xff\xdb", b"\x00\xdc", b"\xff\xdf"])
            elif k == 1: src += b"\x41"
            elif k == 2: src[pos:pos] = b"\x00\xdc\x00\xd8"
            elif k == 3: src += b"\x3d\xd8"
            else: src[pos:pos] = b"\x00\xd8\x41\x00"
            src = bytes(src)
        try: want = src.decode("utf-16-le").encode("utf-8"); ret = len(want)
        except UnicodeDecodeError: want, ret = None, M64
    elif op == 2:
        src = rbytes(rng, rng.choice([0, 1, 2, 3, rng.randrange(0, 100)]))
        want = base64.b64encode(src); ret = len(want)
    elif op == 3:
        src = base64.b64encode(rbytes(rng, rng.choice([0, 1, 2, 3, rng.randrange(0, 70)])))
        if rng.random() < 0.5: src = _b64_corrupt(rng, src)
        try:
            want = base64.b64decode(src, validate=True)
            if base64.b64encode(want) != src: raise ValueError
            ret = len(want)
        except Exception: want, ret = None, M64
    else:
        src = rbytes(rng, rng.randrange(0, 20)); want, ret = None, M64
    n = len(src)
    dst = rbytes(rng, 2 * n + 4)
    def check(r):
        if r["rax"] != ret:
            return f"rax={r['rax']:#x} want {ret:#x} (op={op:#x}, n={n}, src={src[:40].hex()})"
        if want is not None:
            got = r["bufs"][0]
            if got[:len(want)] != want: return f"dst wrong (op={op}, n={n}, src={src[:40].hex()})"
            if got[len(want):] != dst[len(want):]: return f"wrote beyond the result (op={op}, n={n})"
        return None
    return [dst, src], [op, "p0", "p1", n], check

TASKS = {k[2:]: v for k, v in globals().items() if k.startswith("t_")}

def judge_a64(task, binary, cases, seed):
    """AArch64 submissions: run under the independent interpreter a64.py."""
    import a64
    rng = random.Random(seed)
    gen = TASKS[task[:-4]]
    code = open(binary, "rb").read()
    fails = []
    for i in range(cases):
        bufs, args, check = gen(rng)
        try:
            r = a64.call(code, bufs, args, rng)
        except a64.Fault as e:
            fails.append((i, f"{e.kind}: {e}")); continue
        msg = "callee-saved register or sp changed" if not r["saved"] else check(r)
        if msg: fails.append((i, msg))
    return {"task": task, "binary": binary, "seed": seed, "cases": cases, "passed": cases - len(fails),
            "failed": len(fails), "verdict": "PASS" if not fails else "FAIL", "first_failures": fails[:5]}

def judge(task, binary, cases=2000, seed=None):
    if seed is None:
        seed = int.from_bytes(os.urandom(8), "little")
    if task.endswith("_a64"):
        return judge_a64(task, binary, cases, seed)
    rng = random.Random(seed)
    gen = TASKS[task]
    specs, lines = [], []
    for _ in range(cases):
        bufs, args, check = gen(rng)
        specs.append(check)
        bt = " ".join(b.hex() if b else "-" for b in bufs)
        at = " ".join(a if isinstance(a, str) else "i%x" % a for a in args)
        lines.append(f"{len(bufs)} {bt} {len(args)} {at}".replace("  ", " "))
    runner = os.path.join(HERE, "runner")
    out = subprocess.run([runner, binary], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=600).stdout.splitlines()
    fails = []
    for i, (check, line) in enumerate(zip(specs, out)):
        if not line.startswith("ok "):
            fails.append((i, line)); continue
        parts = dict(kv.split("=", 1) for kv in line.split()[1:])
        r = {"rax": int(parts["rax"], 16),
             "bufs": [b"" if x == "-" else bytes.fromhex(x) for x in parts["bufs"].split(",")] if parts["bufs"] else []}
        msg = None
        if parts["saved"] != "1": msg = "callee-saved register changed"
        elif parts["df"] != "0": msg = "DF set on return"
        elif parts["canary"] != "ok": msg = "wrote before the start of a buffer"
        else: msg = check(r)
        if msg: fails.append((i, msg))
    if len(out) != cases:
        fails.append((-1, f"runner produced {len(out)} results for {cases} cases"))
    return {"task": task, "binary": binary, "seed": seed, "cases": cases, "passed": cases - len(fails),
            "failed": len(fails), "verdict": "PASS" if not fails else "FAIL", "first_failures": fails[:5]}

if __name__ == "__main__":
    args = sys.argv[1:]
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 2000
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else None
    print(json.dumps(judge(args[0], args[1], n, seed)))
