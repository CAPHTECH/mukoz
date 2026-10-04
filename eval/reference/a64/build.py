"""Hand encodings of AArch64 reference implementations and mutations.

No AArch64 assembler is installed on the host, so each instruction is encoded
by a small function here (from the Arm ARM encoding tables). The encodings are
cross-checked by running the results under two independent executors:
Mukoz (Unicorn) and eval/oracle/a64.py.

usage: python3 build.py  -> writes ../bin/<name>.bin
"""
import os

EQ, NE, HS, LO, MI, PL, HI, LS, GE, LT, GT, LE = 0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13
XZR = 31


def movz(d, imm, sf=1): return (0xD2800000 if sf else 0x52800000) | (imm << 5) | d
def add_i(d, n, imm, sf=1): return (0x91000000 if sf else 0x11000000) | (imm << 10) | (n << 5) | d
def sub_i(d, n, imm, sf=1): return (0xD1000000 if sf else 0x51000000) | (imm << 10) | (n << 5) | d
def subs_i(d, n, imm, sf=1): return (0xF1000000 if sf else 0x71000000) | (imm << 10) | (n << 5) | d
def cmp_i(n, imm, sf=1): return subs_i(XZR, n, imm, sf)
def add_r(d, n, m, sf=1): return (0x8B000000 if sf else 0x0B000000) | (m << 16) | (n << 5) | d
def subs_r(d, n, m, sf=1): return (0xEB000000 if sf else 0x6B000000) | (m << 16) | (n << 5) | d
def cmp_r(n, m, sf=1): return subs_r(XZR, n, m, sf)
def orr_r(d, n, m, sf=1): return (0xAA000000 if sf else 0x2A000000) | (m << 16) | (n << 5) | d
def mov_r(d, m, sf=1): return orr_r(d, XZR, m, sf)
def ubfm(d, n, immr, imms, sf=1): return (0xD3400000 if sf else 0x53000000) | (immr << 16) | (imms << 10) | (n << 5) | d
def uxtb(d, n): return ubfm(d, n, 0, 7, 0)
def lsr_i(d, n, s, sf=1): return ubfm(d, n, s, 63 if sf else 31, sf)
def ubfx(d, n, lsb, width, sf=1): return ubfm(d, n, lsb, lsb + width - 1, sf)
def lslv(d, n, m): return 0x9AC02000 | (m << 16) | (n << 5) | d
def mul(d, n, m): return 0x9B007C00 | (m << 16) | (n << 5) | d
def csel(d, n, m, c, sf=1): return (0x9A800000 if sf else 0x1A800000) | (m << 16) | (c << 12) | (n << 5) | d
def csinc(d, n, m, c, sf=1): return (0x9A800400 if sf else 0x1A800400) | (m << 16) | (c << 12) | (n << 5) | d
def cinc(d, n, c): return csinc(d, n, n, c ^ 1)
def ldrb_r(t, n, m): return 0x38606800 | (m << 16) | (n << 5) | t
def strb_r(t, n, m): return 0x38206800 | (m << 16) | (n << 5) | t
RET = 0xD65F03C0
NOP = 0xD503201F


class Asm:
    def __init__(self):
        self.items, self.labels = [], {}

    def __call__(self, *words):
        self.items.extend(words)
        return self

    def label(self, name):
        self.labels[name] = len(self.items)

    def b(self, target): self.items.append(("b", target))
    def bcond(self, c, target): self.items.append(("bc", c, target))
    def cbz(self, t, target): self.items.append(("cbz", t, target))
    def bl(self, target): self.items.append(("bl", target))

    def bytes(self):
        out = []
        for idx, it in enumerate(self.items):
            if isinstance(it, int):
                out.append(it)
                continue
            rel = self.labels[it[-1]] - idx
            if it[0] == "bl":
                out.append(0x94000000 | (rel & 0x3FFFFFF))
            elif it[0] == "b":
                out.append(0x14000000 | (rel & 0x3FFFFFF))
            elif it[0] == "bc":
                out.append(0x54000000 | ((rel & 0x7FFFF) << 5) | it[1])
            elif it[0] == "cbz":
                out.append(0xB4000000 | ((rel & 0x7FFFF) << 5) | it[1])
        return b"".join(w.to_bytes(4, "little") for w in out)


def count_byte(mutant=False):
    a = Asm()
    a(NOP if mutant else uxtb(2, 2), movz(3, 0), movz(4, 0))
    a.label("loop"); a(cmp_r(4, 1)); a.bcond(HS, "done")
    a(ldrb_r(5, 0, 4), cmp_r(5, 2, 0), cinc(3, 3, EQ), add_i(4, 4, 1)); a.b("loop")
    a.label("done"); a(mov_r(0, 3), RET)
    return a.bytes()


def memmove(mutant=False):
    a = Asm()
    a(cmp_r(0, 1))
    if mutant: a(NOP)
    else: a.bcond(HI, "back")
    a(movz(3, 0))
    a.label("fwd"); a(cmp_r(3, 2)); a.bcond(HS, "done")
    a(ldrb_r(4, 1, 3), strb_r(4, 0, 3), add_i(3, 3, 1)); a.b("fwd")
    a.label("back"); a.cbz(2, "done")
    a(sub_i(2, 2, 1), ldrb_r(4, 1, 2), strb_r(4, 0, 2)); a.b("back")
    a.label("done"); a(RET)
    return a.bytes()


def isqrt(mutant=False):
    a = Asm()
    a(movz(1, 0), movz(2, 30 if mutant else 31), movz(6, 1))
    a.label("loop")
    a(lslv(3, 6, 2), orr_r(3, 1, 3), mul(4, 3, 3), cmp_r(4, 0), csel(1, 3, 1, LS), subs_i(2, 2, 1))
    a.bcond(PL, "loop")
    a(mov_r(0, 1), RET)
    return a.bytes()


def hex_encode(mutant=False):
    alpha = 0x37 if mutant else 0x57
    a = Asm()
    a(movz(3, 0))
    a.label("loop"); a(cmp_r(3, 2)); a.bcond(HS, "done")
    a(ldrb_r(4, 1, 3), lsr_i(5, 4, 4, 0), ubfx(6, 4, 0, 4, 0))
    a(add_i(7, 5, 0x30, 0), add_i(8, 5, alpha, 0), cmp_i(5, 10, 0), csel(7, 8, 7, HS, 0))
    a(add_r(9, 3, 3), strb_r(7, 0, 9))
    a(add_i(7, 6, 0x30, 0), add_i(8, 6, alpha, 0), cmp_i(6, 10, 0), csel(7, 8, 7, HS, 0))
    a(add_i(9, 9, 1), strb_r(7, 0, 9), add_i(3, 3, 1)); a.b("loop")
    a.label("done"); a(RET)
    return a.bytes()


def lsl_i(d, n, s, sf=1):
    w = 64 if sf else 32
    return ubfm(d, n, (w - s) % w, w - 1 - s, sf)


def base64(mutant=False):
    a = Asm()
    a(mov_r(15, 30), movz(3, 0), movz(4, 0))
    a.label("loop"); a(subs_r(5, 2, 3)); a.bcond(EQ, "done")
    a(ldrb_r(6, 1, 3), movz(7, 0, 0), movz(8, 0, 0), cmp_i(5, 1)); a.bcond(LS, "l1")
    a(add_i(9, 3, 1), ldrb_r(7, 1, 9), cmp_i(5, 2)); a.bcond(LS, "l1")
    a(add_i(9, 3, 2), ldrb_r(8, 1, 9))
    a.label("l1")
    a(lsr_i(10, 6, 2, 0)); a.bl("enc"); a(strb_r(11, 0, 4), add_i(4, 4, 1))
    a(ubfx(10, 6, 0, 2, 0), lsl_i(10, 10, 4, 0), lsr_i(12, 7, 4, 0), orr_r(10, 10, 12, 0)); a.bl("enc")
    a(strb_r(11, 0, 4), add_i(4, 4, 1))
    a(movz(11, 61, 0), cmp_i(5, 1)); a.bcond(LS, "s2")
    a(ubfx(10, 7, 0, 4, 0), lsl_i(10, 10, 2, 0), lsr_i(12, 8, 6, 0), orr_r(10, 10, 12, 0)); a.bl("enc")
    a.label("s2"); a(strb_r(11, 0, 4), add_i(4, 4, 1))
    a(movz(11, 61, 0), cmp_i(5, 1 if mutant else 2)); a.bcond(LS, "s3")
    a(ubfx(10, 8, 0, 6, 0)); a.bl("enc")
    a.label("s3"); a(strb_r(11, 0, 4), add_i(4, 4, 1), add_i(3, 3, 3), cmp_i(5, 3)); a.bcond(HI, "loop")
    a.label("done"); a(mov_r(30, 15), RET)
    a.label("enc")
    a(add_i(11, 10, 65, 0), add_i(12, 10, 71, 0), cmp_i(10, 26, 0), csel(11, 12, 11, HS, 0))
    a(sub_i(12, 10, 4, 0), cmp_i(10, 52, 0), csel(11, 12, 11, HS, 0))
    a(movz(12, 43, 0), cmp_i(10, 62, 0), csel(11, 12, 11, HS, 0))
    a(movz(12, 47, 0), cmp_i(10, 63, 0), csel(11, 12, 11, HS, 0), RET)
    return a.bytes()


MUTANTS = {"base64": "pad", "count_byte": "widecmp", "memmove": "forward_only", "isqrt": "bit30", "hex_encode": "upper"}

if __name__ == "__main__":
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "bin")
    os.makedirs(out, exist_ok=True)
    for name, mut in MUTANTS.items():
        f = globals()[name]
        open(os.path.join(out, f"{name}_a64.bin"), "wb").write(f())
        open(os.path.join(out, f"{name}_a64_mut_{mut}.bin"), "wb").write(f(True))
    print("ok", sorted(MUTANTS))
