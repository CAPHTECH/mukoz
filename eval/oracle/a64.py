"""Minimal A64 (AArch64) integer interpreter for the hidden oracle.

Independent of Mukoz and Unicorn: written from the Arm ARM pseudocode for the
integer subset below. Anything outside the subset is reported as
"unsupported", never as a pass.

Covered: add/sub (imm, shifted, extended), adc/sbc, logical (imm, shifted),
move wide, bitfield, extr, csel family, ccmp/ccmn, 1/2/3-source data
processing, adr/adrp, b/bl/b.cond/cbz/cbnz/tbz/tbnz/br/blr/ret, GPR loads and
stores (unsigned imm, unscaled, pre/post index, register offset, literal,
ldp/stp/ldpsw), hints. SIMD/FP, system registers, exclusives and atomics are
unsupported.
"""

M64 = (1 << 64) - 1


class Fault(Exception):
    def __init__(self, kind, msg):
        super().__init__(msg)
        self.kind = kind


def mask(n):
    return (1 << n) - 1


def sx(v, bits):
    v &= mask(bits)
    return v - (1 << bits) if v >> (bits - 1) else v


def ror(v, r, n):
    r %= n
    v &= mask(n)
    return ((v >> r) | (v << (n - r))) & mask(n)


def replicate(v, esize, n):
    out = 0
    for i in range(0, n, esize):
        out |= v << i
    return out & mask(n)


def decode_bit_masks(N, imms, immr, immediate, datasize):
    combined = (N << 6) | (~imms & 0x3F)
    if combined == 0:
        raise Fault("unsupported", "reserved bitmask")
    ln = combined.bit_length() - 1
    if ln < 1:
        raise Fault("unsupported", "reserved bitmask")
    levels = mask(ln)
    if immediate and (imms & levels) == levels:
        raise Fault("unsupported", "reserved bitmask")
    S = imms & levels
    R = immr & levels
    esize = 1 << ln
    if esize > datasize:
        raise Fault("unsupported", "reserved bitmask")
    d = (S - R) & levels
    welem = mask(S + 1)
    telem = mask(d + 1)
    wmask = replicate(ror(welem, R, esize), esize, datasize)
    tmask = replicate(telem, esize, datasize)
    return wmask, tmask


def add_with_carry(x, y, carry, n):
    usum = (x & mask(n)) + (y & mask(n)) + carry
    ssum = sx(x, n) + sx(y, n) + carry
    res = usum & mask(n)
    N = res >> (n - 1)
    Z = int(res == 0)
    C = int(usum != res)
    V = int(sx(res, n) != ssum)
    return res, (N << 3) | (Z << 2) | (C << 1) | V


class Machine:
    def __init__(self, code, code_base, regions, stack_lo, stack_hi):
        """regions: list of [base, bytearray, writable]."""
        self.code = code
        self.code_base = code_base
        self.regions = regions
        self.stack_lo, self.stack_hi = stack_lo, stack_hi
        self.stack = bytearray(stack_hi - stack_lo)
        self.x = [0] * 31
        self.sp = 0
        self.pc = code_base
        self.nzcv = 0

    # ---- registers
    def r(self, n, sf, sp=False):
        if n == 31:
            v = self.sp if sp else 0
        else:
            v = self.x[n]
        return v if sf else v & 0xFFFFFFFF

    def w(self, n, v, sf, sp=False):
        v &= M64 if sf else 0xFFFFFFFF
        if n == 31:
            if sp:
                self.sp = v
        else:
            self.x[n] = v

    # ---- memory
    def _locate(self, addr, size, write):
        if self.stack_lo <= addr and addr + size <= self.stack_hi:
            return self.stack, addr - self.stack_lo
        for base, buf, writable in self.regions:
            if base <= addr and addr + size <= base + len(buf):
                if write and not writable:
                    raise Fault("memory", f"write to read-only buffer at {addr:#x}")
                return buf, addr - base
        if not write and self.code_base <= addr and addr + size <= self.code_base + len(self.code):
            return bytes(self.code), addr - self.code_base
        raise Fault("memory", f"{'write' if write else 'read'} of {size} bytes at {addr:#x} outside allowed memory")

    def load(self, addr, size):
        buf, off = self._locate(addr & M64, size, False)
        return int.from_bytes(buf[off:off + size], "little")

    def store(self, addr, size, v):
        buf, off = self._locate(addr & M64, size, True)
        buf[off:off + size] = (v & mask(8 * size)).to_bytes(size, "little")

    # ---- helpers
    def cond(self, c):
        N, Z, C, V = (self.nzcv >> 3) & 1, (self.nzcv >> 2) & 1, (self.nzcv >> 1) & 1, self.nzcv & 1
        b = c >> 1
        if b == 0: res = Z == 1
        elif b == 1: res = C == 1
        elif b == 2: res = N == 1
        elif b == 3: res = V == 1
        elif b == 4: res = C == 1 and Z == 0
        elif b == 5: res = N == V
        elif b == 6: res = N == V and Z == 0
        else: res = True
        if c & 1 and c != 0xF:
            res = not res
        return res

    def shift(self, v, typ, amt, n):
        v &= mask(n)
        if typ == 0: return (v << amt) & mask(n)
        if typ == 1: return v >> amt
        if typ == 2: return (sx(v, n) >> amt) & mask(n)
        return ror(v, amt, n)

    def extend(self, v, option, amt, n):
        size = [8, 16, 32, 64][option & 3]
        v &= mask(size)
        if option & 4:
            v = sx(v, size) & M64
        return (v << amt) & mask(n)

    # ---- execution
    def run(self, sentinel, budget):
        steps = 0
        while self.pc != sentinel:
            if steps >= budget:
                raise Fault("budget", "instruction budget exhausted")
            steps += 1
            off = self.pc - self.code_base
            if off < 0 or off + 4 > len(self.code) or off % 4:
                raise Fault("left_code", f"pc {self.pc:#x} outside code")
            insn = int.from_bytes(self.code[off:off + 4], "little")
            self.step(insn)
        return steps

    def step(self, i):
        pc = self.pc
        nxt = pc + 4
        bits = lambda hi, lo: (i >> lo) & mask(hi - lo + 1)
        sf = i >> 31
        n = 64 if sf else 32
        Rd, Rn, Rm = i & 31, (i >> 5) & 31, (i >> 16) & 31

        if (i & 0x1F800000) == 0x11000000:  # add/sub immediate
            op, S, sh = (i >> 30) & 1, (i >> 29) & 1, (i >> 22) & 1
            imm = bits(21, 10) << (12 if sh else 0)
            a = self.r(Rn, sf, sp=True)
            res, fl = add_with_carry(a, (~imm if op else imm), op, n)
            if S: self.nzcv = fl
            self.w(Rd, res, sf, sp=not S)
        elif (i & 0x1F800000) == 0x12000000:  # logical immediate
            opc, N = bits(30, 29), (i >> 22) & 1
            if not sf and N:
                raise Fault("unsupported", "reserved logical immediate")
            imm, _ = decode_bit_masks(N, bits(15, 10), bits(21, 16), True, n)
            a = self.r(Rn, sf)
            res = [a & imm, a | imm, a ^ imm, a & imm][opc]
            if opc == 3:
                self.nzcv = ((res >> (n - 1)) << 3) | (int(res == 0) << 2)
            self.w(Rd, res, sf, sp=opc != 3)
        elif (i & 0x1F800000) == 0x12800000:  # move wide
            opc, hw = bits(30, 29), bits(22, 21)
            if opc == 1 or (not sf and hw > 1):
                raise Fault("unsupported", "reserved move wide")
            imm = bits(20, 5) << (16 * hw)
            if opc == 0: res = ~imm
            elif opc == 2: res = imm
            else: res = (self.r(Rd, sf) & ~(0xFFFF << (16 * hw))) | imm
            self.w(Rd, res, sf)
        elif (i & 0x1F800000) == 0x13000000:  # bitfield
            opc, N = bits(30, 29), (i >> 22) & 1
            if opc == 3 or N != sf:
                raise Fault("unsupported", "reserved bitfield")
            R, S = bits(21, 16), bits(15, 10)
            wmask, tmask = decode_bit_masks(N, S, R, False, n)
            src = self.r(Rn, sf)
            if opc == 1:
                dst = self.r(Rd, sf)
                bot = (dst & ~wmask) | (ror(src, R, n) & wmask)
                res = (dst & ~tmask) | (bot & tmask)
            elif opc == 0:
                bot = ror(src, R, n) & wmask
                top = mask(n) if (src >> S) & 1 else 0
                res = (top & ~tmask) | (bot & tmask)
            else:
                res = ror(src, R, n) & wmask & tmask
            self.w(Rd, res, sf)
        elif (i & 0x1FA00000) == 0x13800000:  # extr
            lsb = bits(15, 10)
            cat = (self.r(Rn, sf) << n) | self.r(Rm, sf)
            self.w(Rd, cat >> lsb, sf)
        elif (i & 0x1F000000) == 0x0A000000:  # logical shifted register
            opc, typ, inv, amt = bits(30, 29), bits(23, 22), (i >> 21) & 1, bits(15, 10)
            if not sf and amt >= 32:
                raise Fault("unsupported", "reserved shift")
            b = self.shift(self.r(Rm, sf), typ, amt, n)
            if inv: b = ~b & mask(n)
            a = self.r(Rn, sf)
            res = [a & b, a | b, a ^ b, a & b][opc]
            if opc == 3:
                self.nzcv = ((res >> (n - 1)) << 3) | (int(res == 0) << 2)
            self.w(Rd, res, sf)
        elif (i & 0x1F200000) == 0x0B000000:  # add/sub shifted register
            op, S, typ, amt = (i >> 30) & 1, (i >> 29) & 1, bits(23, 22), bits(15, 10)
            if typ == 3 or (not sf and amt >= 32):
                raise Fault("unsupported", "reserved shift")
            b = self.shift(self.r(Rm, sf), typ, amt, n)
            res, fl = add_with_carry(self.r(Rn, sf), (~b if op else b), op, n)
            if S: self.nzcv = fl
            self.w(Rd, res, sf)
        elif (i & 0x1F200000) == 0x0B200000:  # add/sub extended register
            op, S, option, amt = (i >> 30) & 1, (i >> 29) & 1, bits(15, 13), bits(12, 10)
            if amt > 4:
                raise Fault("unsupported", "reserved extend")
            b = self.extend(self.r(Rm, 1), option, amt, n)
            res, fl = add_with_carry(self.r(Rn, sf, sp=True), (~b if op else b), op, n)
            if S: self.nzcv = fl
            self.w(Rd, res, sf, sp=not S)
        elif (i & 0x1FE0FC00) == 0x1A000000:  # adc/sbc
            op, S = (i >> 30) & 1, (i >> 29) & 1
            b = self.r(Rm, sf)
            res, fl = add_with_carry(self.r(Rn, sf), (~b if op else b), (self.nzcv >> 1) & 1, n)
            if S: self.nzcv = fl
            self.w(Rd, res, sf)
        elif (i & 0x1FE00410) == 0x1A400000:  # ccmn/ccmp (reg and imm)
            op, c = (i >> 30) & 1, bits(15, 12)
            if self.cond(c):
                b = Rm if (i >> 11) & 1 else self.r(Rm, sf)
                _, fl = add_with_carry(self.r(Rn, sf), (~b if op else b), op, n)
                self.nzcv = fl
            else:
                self.nzcv = i & 0xF
        elif (i & 0x1FE00800) == 0x1A800000:  # csel family
            op, op2, c = (i >> 30) & 1, bits(11, 10), bits(15, 12)
            if self.cond(c):
                res = self.r(Rn, sf)
            else:
                res = self.r(Rm, sf)
                if op: res = ~res
                if op2 & 1: res += 1
            self.w(Rd, res, sf)
        elif (i & 0x5FE00000) == 0x1AC00000:  # 2-source
            opcode = bits(15, 10)
            a, b = self.r(Rn, sf), self.r(Rm, sf)
            if opcode == 2: res = 0 if b == 0 else a // b
            elif opcode == 3:
                sa, sb = sx(a, n), sx(b, n)
                if sb == 0: res = 0
                else:
                    q = abs(sa) // abs(sb)
                    res = q if (sa < 0) == (sb < 0) else -q
            elif opcode == 8: res = a << (b % n)
            elif opcode == 9: res = a >> (b % n)
            elif opcode == 10: res = sx(a, n) >> (b % n)
            elif opcode == 11: res = ror(a, b % n, n)
            else: raise Fault("unsupported", f"2-source opcode {opcode}")
            self.w(Rd, res, sf)
        elif (i & 0x5FE00000) == 0x5AC00000:  # 1-source
            opcode = bits(15, 10)
            a = self.r(Rn, sf)
            if opcode == 0: res = int(format(a, f"0{n}b")[::-1], 2)
            elif opcode in (1, 2, 3):
                cs = {1: 16, 2: 32, 3: 64}[opcode]
                if cs > n: raise Fault("unsupported", "reserved rev")
                res = 0
                for base in range(0, n, cs):
                    chunk = (a >> base) & mask(cs)
                    res |= int.from_bytes(chunk.to_bytes(cs // 8, "little"), "big") << base
            elif opcode == 4: res = n - a.bit_length()
            elif opcode == 5:
                top = a >> (n - 1)
                v = a ^ (mask(n) if top else 0)
                res = n - v.bit_length() - 1
            else: raise Fault("unsupported", f"1-source opcode {opcode}")
            self.w(Rd, res, sf)
        elif (i & 0x1F000000) == 0x1B000000:  # 3-source
            op31, o0, Ra = bits(23, 21), (i >> 15) & 1, bits(14, 10)
            a, b = self.r(Rn, sf), self.r(Rm, sf)
            if op31 == 0:
                acc = self.r(Ra, sf)
                res = acc - a * b if o0 else acc + a * b
            elif op31 in (1, 5) and sf:
                if op31 == 1: p = sx(a, 32) * sx(b, 32)
                else: p = (a & 0xFFFFFFFF) * (b & 0xFFFFFFFF)
                acc = self.r(Ra, 1)
                res = acc - p if o0 else acc + p
            elif op31 == 2 and sf and not o0: res = (sx(a, 64) * sx(b, 64)) >> 64
            elif op31 == 6 and sf and not o0: res = (a * b) >> 64
            else: raise Fault("unsupported", "3-source variant")
            self.w(Rd, res, sf)
        elif (i & 0x1F000000) == 0x10000000:  # adr/adrp
            imm = sx((bits(23, 5) << 2) | bits(30, 29), 21)
            if sf: res = (pc & ~0xFFF) + (imm << 12)
            else: res = pc + imm
            self.w(Rd, res, 1)
        elif (i & 0x7C000000) == 0x14000000:  # b / bl
            if sf: self.x[30] = nxt
            nxt = pc + (sx(bits(25, 0), 26) << 2)
        elif (i & 0xFF000010) == 0x54000000:  # b.cond
            if self.cond(i & 0xF):
                nxt = pc + (sx(bits(23, 5), 19) << 2)
        elif (i & 0x7E000000) == 0x34000000:  # cbz/cbnz
            v = self.r(Rd, sf)
            if (v == 0) != bool((i >> 24) & 1):
                nxt = pc + (sx(bits(23, 5), 19) << 2)
        elif (i & 0x7E000000) == 0x36000000:  # tbz/tbnz
            bit = (sf << 5) | bits(23, 19)
            v = (self.x[Rd] if Rd != 31 else 0) >> bit & 1
            if v == (i >> 24) & 1:
                nxt = pc + (sx(bits(18, 5), 14) << 2)
        elif (i & 0xFF9FFC1F) == 0xD61F0000:  # br / blr / ret
            opc = bits(22, 21)
            target = self.r(Rn, 1)
            if opc == 1: self.x[30] = nxt
            nxt = target
        elif (i & 0xFFFFF01F) == 0xD503201F:  # hints (nop etc.)
            pass
        elif (i & 0xFFE0001F) == 0xD4000001:  # svc
            raise Fault("forbidden", "system call")
        elif (i & 0x3B000000) == 0x18000000:  # load literal
            if (i >> 26) & 1: raise Fault("unsupported", "SIMD/FP load")
            opc = bits(31, 30)
            addr = pc + (sx(bits(23, 5), 19) << 2)
            if opc == 0: self.w(Rd, self.load(addr, 4), 1)
            elif opc == 1: self.w(Rd, self.load(addr, 8), 1)
            elif opc == 2: self.w(Rd, sx(self.load(addr, 4), 32), 1)
        elif (i & 0x3A000000) == 0x28000000:  # load/store pair
            if (i >> 26) & 1: raise Fault("unsupported", "SIMD/FP pair")
            opc, mode, L = bits(31, 30), bits(24, 23), (i >> 22) & 1
            Rt2 = bits(14, 10)
            if opc == 3 or (opc == 1 and not L): raise Fault("unsupported", "reserved pair")
            size = 8 if opc == 2 else 4
            off = sx(bits(21, 15), 7) * size
            base = self.r(Rn, 1, sp=True)
            addr = base if mode == 1 else base + off
            if L:
                a = self.load(addr, size); b = self.load(addr + size, size)
                if opc == 1: a, b = sx(a, 32), sx(b, 32)
                self.w(Rd, a, 1); self.w(Rt2, b, 1)
            else:
                self.store(addr, size, self.r(Rd, 1)); self.store(addr + size, size, self.r(Rt2, 1))
            if mode in (1, 3):
                self.w(Rn, base + off, 1, sp=True)
        elif (i & 0x3B000000) == 0x39000000 or (i & 0x3B200000) == 0x38000000 or (i & 0x3B200C00) == 0x38200800:
            if (i >> 26) & 1: raise Fault("unsupported", "SIMD/FP load/store")
            size, opc = bits(31, 30), bits(23, 22)
            nb = 1 << size
            wb = None
            base = self.r(Rn, 1, sp=True)
            if (i & 0x3B000000) == 0x39000000:
                addr = base + (bits(21, 10) << size)
            elif (i & 0x3B200C00) == 0x38200800:
                option, S = bits(15, 13), (i >> 12) & 1
                if not option & 2: raise Fault("unsupported", "reserved extend")
                addr = base + self.extend(self.r(Rm, 1), option, size if S else 0, 64)
            else:
                imm = sx(bits(20, 12), 9)
                mode = bits(11, 10)
                if mode == 1: addr, wb = base, base + imm
                elif mode == 3: addr = wb = base + imm
                else: addr = base + imm
            addr &= M64
            if opc == 0:
                self.store(addr, nb, self.r(Rd, 1))
            elif opc == 1:
                self.w(Rd, self.load(addr, nb), 1)
            elif size == 3 and opc == 2:
                pass  # prfm
            elif opc == 2:
                self.w(Rd, sx(self.load(addr, nb), 8 * nb), 1)
            elif size <= 1:
                self.w(Rd, sx(self.load(addr, nb), 8 * nb), 0)
            else:
                raise Fault("unsupported", "reserved load")
            if wb is not None:
                self.w(Rn, wb, 1, sp=True)
        else:
            raise Fault("unsupported", f"instruction {i:08x} at offset {pc - self.code_base:#x}")
        self.pc = nxt & M64


CODE_BASE = 0x400000
BUF_BASE = 0x10000000
BUF_STRIDE = 0x100000
STACK_HI = 0x7FFF0000
SENTINEL = 0x0DEAD000


def call(code, bufs, args, rng, budget=2_000_000):
    """Run code like a native AAPCS64 call. bufs: list of bytes (writable);
    args: ints, or strings 'pK' / 'pK+off'. Returns a result dict like the
    native runner: x0, bufs, saved (callee-saved and sp restored), or raises Fault."""
    regions = [[BUF_BASE + k * BUF_STRIDE, bytearray(b), True] for k, b in enumerate(bufs)]
    entry_sp = STACK_HI - 0x100
    m = Machine(code, CODE_BASE, regions, STACK_HI - 0x10000, entry_sp)
    for k in range(31):
        m.x[k] = rng.getrandbits(64)
    for k, a in enumerate(args):
        if isinstance(a, str):
            idx, _, off = a[1:].partition("+")
            m.x[k] = regions[int(idx)][0] + (int(off) if off else 0)
        else:
            m.x[k] = a & M64
    m.x[30] = SENTINEL
    m.sp = entry_sp
    saved = m.x[19:30]
    m.run(SENTINEL, budget)
    return {"rax": m.x[0], "bufs": [bytes(r[1]) for r in regions],
            "saved": m.x[19:30] == saved and m.sp == entry_sp}
